use std::{
    sync::{
        Arc,
        atomic::{AtomicU8, AtomicU64, Ordering},
    },
    time::Duration,
};

use crate::camera::{
    CaptureAdapter, CaptureAdapterError, CaptureOpen, DeviceEndpoint, DeviceEndpointKey,
    DeviceScanPolicy, FrameRead, MonotonicClock, ProbeOutcome, ProbeStatus, service::ServiceCore,
};

const CANCEL_NONE: u8 = 0;
const CANCEL_USER: u8 = 1;
const CANCEL_OPERATION_DEADLINE: u8 = 2;
const EMPTY_FRAME_RETRY_DELAY: Duration = Duration::from_millis(10);

pub(crate) struct OperationControl {
    cancel_reason: AtomicU8,
    probe_timeout_epoch: AtomicU64,
}

impl OperationControl {
    pub(crate) fn new() -> Self {
        Self {
            cancel_reason: AtomicU8::new(CANCEL_NONE),
            probe_timeout_epoch: AtomicU64::new(0),
        }
    }

    pub(crate) fn request_user_cancel(&self) {
        let _ = self.cancel_reason.compare_exchange(
            CANCEL_NONE,
            CANCEL_USER,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }

    pub(crate) fn request_operation_timeout(&self) {
        let _ = self.cancel_reason.compare_exchange(
            CANCEL_NONE,
            CANCEL_OPERATION_DEADLINE,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }

    pub(crate) fn cancel_reason(&self) -> CancelReason {
        match self.cancel_reason.load(Ordering::Acquire) {
            CANCEL_USER => CancelReason::User,
            CANCEL_OPERATION_DEADLINE => CancelReason::OperationDeadline,
            _ => CancelReason::None,
        }
    }

    pub(crate) fn request_probe_timeout(&self, epoch: u64) {
        self.probe_timeout_epoch.store(epoch, Ordering::Release);
    }

    pub(crate) fn probe_timed_out(&self, epoch: u64) -> bool {
        self.probe_timeout_epoch.load(Ordering::Acquire) == epoch
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CancelReason {
    None,
    User,
    OperationDeadline,
}

pub(crate) enum WorkerResult {
    Completed,
    Cancelled,
    OperationTimedOut,
    ReleaseFailed(CaptureAdapterError),
}

pub(crate) struct WorkerContext {
    pub(crate) core: Arc<ServiceCore>,
    pub(crate) control: Arc<OperationControl>,
    pub(crate) clock: Arc<dyn MonotonicClock>,
    pub(crate) policy: DeviceScanPolicy,
    pub(crate) service_instance: u64,
    pub(crate) generation: u64,
}

pub(crate) fn run_worker(
    mut adapter: Box<dyn CaptureAdapter>,
    context: WorkerContext,
) -> WorkerResult {
    let mut epoch = 0_u64;
    let mut targets = context.policy.targets().into_iter().peekable();
    while let Some(target) = targets.next() {
        if let Some(result) = cancellation_result(&context.control) {
            return result;
        }
        epoch = match epoch.checked_add(1) {
            Some(value) => value,
            None => return WorkerResult::OperationTimedOut,
        };
        let probe_started_at = context.clock.now();
        context.core.begin_probe(target, epoch, probe_started_at);

        let status = match adapter.open(target) {
            Ok(CaptureOpen::Unavailable) => {
                probe_status_after_open(&context, epoch, probe_started_at, ProbeStatus::OpenFailed)
            }
            Err(error @ CaptureAdapterError::Release { .. }) => {
                return WorkerResult::ReleaseFailed(error);
            }
            Err(_) => {
                probe_status_after_open(&context, epoch, probe_started_at, ProbeStatus::OpenFailed)
            }
            Ok(CaptureOpen::Session(mut session)) => {
                let status = read_first_frame(session.as_mut(), &context, epoch, probe_started_at);
                if let Err(error) = session.release() {
                    return WorkerResult::ReleaseFailed(error);
                }
                status
            }
        };

        let outcome = ProbeOutcome::new(target, status);
        let endpoint = if status == ProbeStatus::Available {
            DeviceEndpoint::from_available_outcome(
                DeviceEndpointKey::new(context.service_instance, context.generation, target),
                context.generation,
                outcome,
            )
        } else {
            None
        };
        context
            .core
            .complete_probe(outcome, endpoint, context.clock.now());

        if status == ProbeStatus::Cancelled {
            return cancellation_result(&context.control).unwrap_or(WorkerResult::Cancelled);
        }
        if let Some(result) = cancellation_result(&context.control) {
            return result;
        }
        if targets.peek().is_some() && !interruptible_delay(&context, context.policy.reopen_delay())
        {
            return cancellation_result(&context.control).unwrap_or(WorkerResult::Cancelled);
        }
    }
    WorkerResult::Completed
}

fn read_first_frame(
    session: &mut dyn crate::camera::CaptureSession,
    context: &WorkerContext,
    epoch: u64,
    probe_started_at: Duration,
) -> ProbeStatus {
    loop {
        if context.control.cancel_reason() != CancelReason::None {
            return ProbeStatus::Cancelled;
        }
        if context.control.probe_timed_out(epoch)
            || context.clock.now().saturating_sub(probe_started_at)
                >= context.policy.first_frame_deadline()
        {
            return ProbeStatus::FirstFrameTimeout;
        }
        let read = session.read();
        if context.control.cancel_reason() != CancelReason::None {
            return ProbeStatus::Cancelled;
        }
        if context.control.probe_timed_out(epoch)
            || context.clock.now().saturating_sub(probe_started_at)
                >= context.policy.first_frame_deadline()
        {
            return ProbeStatus::FirstFrameTimeout;
        }
        match read {
            Ok(FrameRead::Frame(_)) => return ProbeStatus::Available,
            Err(_) => return ProbeStatus::ReadFailed,
            Ok(FrameRead::Empty) => {
                context.core.note_progress(context.clock.now());
                let remaining = context
                    .policy
                    .first_frame_deadline()
                    .saturating_sub(context.clock.now().saturating_sub(probe_started_at));
                context.clock.sleep(EMPTY_FRAME_RETRY_DELAY.min(remaining));
            }
        }
    }
}

fn probe_status_after_open(
    context: &WorkerContext,
    epoch: u64,
    probe_started_at: Duration,
    ordinary_status: ProbeStatus,
) -> ProbeStatus {
    if context.control.cancel_reason() != CancelReason::None {
        ProbeStatus::Cancelled
    } else if context.control.probe_timed_out(epoch)
        || context.clock.now().saturating_sub(probe_started_at)
            >= context.policy.first_frame_deadline()
    {
        ProbeStatus::FirstFrameTimeout
    } else {
        ordinary_status
    }
}

fn interruptible_delay(context: &WorkerContext, duration: Duration) -> bool {
    let started_at = context.clock.now();
    while context.clock.now().saturating_sub(started_at) < duration {
        if context.control.cancel_reason() != CancelReason::None {
            return false;
        }
        let remaining = duration.saturating_sub(context.clock.now().saturating_sub(started_at));
        context.clock.sleep(EMPTY_FRAME_RETRY_DELAY.min(remaining));
    }
    true
}

fn cancellation_result(control: &OperationControl) -> Option<WorkerResult> {
    match control.cancel_reason() {
        CancelReason::None => None,
        CancelReason::User => Some(WorkerResult::Cancelled),
        CancelReason::OperationDeadline => Some(WorkerResult::OperationTimedOut),
    }
}
