use std::{sync::Arc, time::Duration};

use crate::camera::{
    CaptureAdapter, CaptureAdapterError, CaptureOpen, FrameRead, MonotonicClock, ProbeTarget,
    profiling::{
        CandidateFailureReason, CandidatePhase, CandidateResult, CandidateRetryReason,
        CaptureMetricsAccumulator, CaptureModeStatus, ModeCandidatePolicy, ModeTuple,
        evaluate_capture_gates,
    },
    service::ServiceCore,
    worker::{CancelReason, OperationControl},
};

const EMPTY_FRAME_RETRY_DELAY: Duration = Duration::from_millis(1);

pub(crate) struct ProfileWorkerContext {
    pub(crate) core: Arc<ServiceCore>,
    pub(crate) control: Arc<OperationControl>,
    pub(crate) clock: Arc<dyn MonotonicClock>,
    pub(crate) policy: ModeCandidatePolicy,
    pub(crate) target: ProbeTarget,
}

pub(crate) enum ProfileWorkerResult {
    Completed(Vec<CandidateResult>),
    Cancelled(Vec<CandidateResult>),
    OperationTimedOut(Vec<CandidateResult>),
    ReleaseFailed {
        results: Vec<CandidateResult>,
        error: CaptureAdapterError,
    },
}

pub(crate) fn run_profile_worker(
    mut adapter: Box<dyn CaptureAdapter>,
    context: ProfileWorkerContext,
) -> ProfileWorkerResult {
    let candidates = context.policy.candidates();
    let mut results = Vec::with_capacity(candidates.len());
    for (ordinal, tuple) in candidates.into_iter().enumerate() {
        if let Some(result) = terminal_for_cancel(&context.control, &results) {
            return result;
        }
        let epoch = u64::try_from(ordinal).unwrap_or(u64::MAX).saturating_add(1);
        let candidate_started_at = context.clock.now();
        context
            .core
            .begin_profile_candidate(ordinal, tuple, epoch, candidate_started_at);

        let first = run_candidate_attempt(
            adapter.as_mut(),
            &context,
            ordinal,
            tuple,
            epoch,
            candidate_started_at,
        );
        let mut result = match first {
            AttemptOutcome::Result(result) => *result,
            AttemptOutcome::Retry(reason) => {
                if !interruptible_delay(&context, context.policy.reopen_delay()) {
                    let result = cancelled_result(ordinal, tuple, 1, Some(reason));
                    context
                        .core
                        .complete_profile_candidate(result.clone(), context.clock.now());
                    results.push(result);
                    return terminal_for_cancel(&context.control, &results)
                        .unwrap_or(ProfileWorkerResult::Cancelled(Vec::new()));
                }
                context
                    .core
                    .set_profile_phase(CandidatePhase::Opening, context.clock.now());
                match run_candidate_attempt(
                    adapter.as_mut(),
                    &context,
                    ordinal,
                    tuple,
                    epoch,
                    candidate_started_at,
                ) {
                    AttemptOutcome::Result(mut result) => {
                        result.set_attempt(2, Some(reason));
                        *result
                    }
                    AttemptOutcome::Retry(second_reason) => CandidateResult::failed(
                        ordinal,
                        tuple,
                        2,
                        Some(reason),
                        status_for_retry(second_reason),
                        vec![failure_for_retry(second_reason)],
                    ),
                    AttemptOutcome::ReleaseFailed(error) => {
                        return ProfileWorkerResult::ReleaseFailed { results, error };
                    }
                }
            }
            AttemptOutcome::ReleaseFailed(error) => {
                return ProfileWorkerResult::ReleaseFailed { results, error };
            }
        };

        if context.control.cancel_reason() != CancelReason::None {
            result = cancelled_result(ordinal, tuple, result.attempt_count, result.retry_reason);
        }
        context
            .core
            .complete_profile_candidate(result.clone(), context.clock.now());
        results.push(result);

        if let Some(result) = terminal_for_cancel(&context.control, &results) {
            return result;
        }
        if ordinal + 1 < context.policy.candidate_count()
            && !interruptible_delay(&context, context.policy.reopen_delay())
        {
            return terminal_for_cancel(&context.control, &results)
                .unwrap_or(ProfileWorkerResult::Cancelled(Vec::new()));
        }
    }
    ProfileWorkerResult::Completed(results)
}

enum AttemptOutcome {
    Result(Box<CandidateResult>),
    Retry(CandidateRetryReason),
    ReleaseFailed(CaptureAdapterError),
}

fn run_candidate_attempt(
    adapter: &mut dyn CaptureAdapter,
    context: &ProfileWorkerContext,
    ordinal: usize,
    tuple: ModeTuple,
    epoch: u64,
    candidate_started_at: Duration,
) -> AttemptOutcome {
    if timed_out(context, epoch, candidate_started_at) {
        return AttemptOutcome::Result(Box::new(timed_out_result(ordinal, tuple)));
    }
    let mut session = match adapter.open(context.target) {
        Ok(CaptureOpen::Session(session)) => session,
        Ok(CaptureOpen::Unavailable)
        | Err(CaptureAdapterError::Open { .. })
        | Err(CaptureAdapterError::OpenState { .. }) => {
            return AttemptOutcome::Retry(CandidateRetryReason::OpenFailed);
        }
        Err(error) => return AttemptOutcome::ReleaseFailed(error),
    };

    context
        .core
        .set_profile_phase(CandidatePhase::ApplyingProperties, context.clock.now());
    let set_diagnostics = match session.apply_mode(tuple) {
        Ok(value) => value,
        Err(_) => {
            return release_then(
                session.as_mut(),
                CandidateResult::failed(
                    ordinal,
                    tuple,
                    1,
                    None,
                    CaptureModeStatus::ApplyingFailed,
                    vec![CandidateFailureReason::ApplyFailed],
                ),
            );
        }
    };

    context.core.set_profile_phase(
        CandidatePhase::ReadingReportedProperties,
        context.clock.now(),
    );
    let reported = match session.reported_properties() {
        Ok(value) => value,
        Err(_) => {
            return release_then(
                session.as_mut(),
                CandidateResult::failed(
                    ordinal,
                    tuple,
                    1,
                    None,
                    CaptureModeStatus::ReadFailed,
                    vec![CandidateFailureReason::ReadFailed],
                ),
            );
        }
    };

    context
        .core
        .set_profile_phase(CandidatePhase::FirstFrame, context.clock.now());
    let first_frame_started_at = context.clock.now();
    loop {
        if cancelled(context) {
            return release_then(session.as_mut(), cancelled_result(ordinal, tuple, 1, None));
        }
        if timed_out(context, epoch, candidate_started_at)
            || context.clock.now().saturating_sub(first_frame_started_at)
                >= context.policy.first_frame_deadline()
        {
            return release_then_retry(
                session.as_mut(),
                CandidateRetryReason::FirstReadStartFailed,
            );
        }
        match session.read() {
            Ok(FrameRead::Frame(_)) => break,
            Ok(FrameRead::Empty) => {
                context.core.note_progress(context.clock.now());
                context.clock.sleep(EMPTY_FRAME_RETRY_DELAY);
            }
            Err(_) => {
                return release_then_retry(
                    session.as_mut(),
                    CandidateRetryReason::FirstReadStartFailed,
                );
            }
        }
    }

    context
        .core
        .set_profile_phase(CandidatePhase::Warmup, context.clock.now());
    let warmup_started_at = context.clock.now();
    while context.clock.now().saturating_sub(warmup_started_at) < context.policy.warmup() {
        if cancelled(context) {
            return release_then(session.as_mut(), cancelled_result(ordinal, tuple, 1, None));
        }
        if timed_out(context, epoch, candidate_started_at) {
            return release_then(session.as_mut(), timed_out_result(ordinal, tuple));
        }
        match session.read() {
            Ok(FrameRead::Empty) => context.clock.sleep(EMPTY_FRAME_RETRY_DELAY),
            Ok(FrameRead::Frame(_)) => {}
            Err(_) => {
                return release_then(
                    session.as_mut(),
                    CandidateResult::failed(
                        ordinal,
                        tuple,
                        1,
                        None,
                        CaptureModeStatus::ReadFailed,
                        vec![CandidateFailureReason::ReadFailed],
                    ),
                );
            }
        }
        context.core.note_progress(context.clock.now());
    }

    context
        .core
        .set_profile_phase(CandidatePhase::Measuring, context.clock.now());
    let measurement_started_at = context.clock.now();
    let mut accumulator = CaptureMetricsAccumulator::default();
    while context.clock.now().saturating_sub(measurement_started_at) < context.policy.capture_only()
    {
        if cancelled(context) {
            return release_then(session.as_mut(), cancelled_result(ordinal, tuple, 1, None));
        }
        if timed_out(context, epoch, candidate_started_at) {
            return release_then(session.as_mut(), timed_out_result(ordinal, tuple));
        }
        let read = session.read();
        let now = context.clock.now();
        match read {
            Ok(FrameRead::Frame(metadata)) => {
                if accumulator
                    .note_frame(now.saturating_sub(measurement_started_at), metadata)
                    .is_err()
                {
                    return release_then(
                        session.as_mut(),
                        CandidateResult::failed(
                            ordinal,
                            tuple,
                            1,
                            None,
                            CaptureModeStatus::CaptureUnstable,
                            vec![CandidateFailureReason::ReadFailureRatio],
                        ),
                    );
                }
            }
            Ok(FrameRead::Empty) => {
                accumulator.note_empty();
                context.clock.sleep(EMPTY_FRAME_RETRY_DELAY);
            }
            Err(_) => accumulator.note_failure(),
        }
        context.core.note_progress(now);
    }
    let elapsed = context.clock.now().saturating_sub(measurement_started_at);
    let metrics = match accumulator.finish(elapsed, tuple.requested_fps) {
        Ok(value) => value,
        Err(_) => {
            return release_then(
                session.as_mut(),
                CandidateResult::failed(
                    ordinal,
                    tuple,
                    1,
                    None,
                    CaptureModeStatus::CaptureUnstable,
                    vec![CandidateFailureReason::ReadFailureRatio],
                ),
            );
        }
    };
    context
        .core
        .set_profile_phase(CandidatePhase::Release, context.clock.now());
    if let Err(error) = session.release() {
        return AttemptOutcome::ReleaseFailed(error);
    }
    let evaluation = evaluate_capture_gates(tuple, reported, &metrics, context.policy.thresholds());
    AttemptOutcome::Result(Box::new(CandidateResult::from_terminal_gate(
        ordinal,
        tuple,
        set_diagnostics,
        reported,
        metrics,
        evaluation,
    )))
}

fn release_then(
    session: &mut dyn crate::camera::CaptureSession,
    result: CandidateResult,
) -> AttemptOutcome {
    match session.release() {
        Ok(()) => AttemptOutcome::Result(Box::new(result)),
        Err(error) => AttemptOutcome::ReleaseFailed(error),
    }
}

fn release_then_retry(
    session: &mut dyn crate::camera::CaptureSession,
    reason: CandidateRetryReason,
) -> AttemptOutcome {
    match session.release() {
        Ok(()) => AttemptOutcome::Retry(reason),
        Err(error) => AttemptOutcome::ReleaseFailed(error),
    }
}

fn cancelled(context: &ProfileWorkerContext) -> bool {
    context.control.cancel_reason() != CancelReason::None
}

fn timed_out(context: &ProfileWorkerContext, epoch: u64, candidate_started_at: Duration) -> bool {
    context.control.probe_timed_out(epoch)
        || context.clock.now().saturating_sub(candidate_started_at)
            >= context.policy.candidate_deadline()
}

fn interruptible_delay(context: &ProfileWorkerContext, duration: Duration) -> bool {
    context
        .core
        .set_profile_phase(CandidatePhase::ReopenDelay, context.clock.now());
    let started_at = context.clock.now();
    while context.clock.now().saturating_sub(started_at) < duration {
        if cancelled(context) {
            return false;
        }
        let remaining = duration.saturating_sub(context.clock.now().saturating_sub(started_at));
        context.clock.sleep(EMPTY_FRAME_RETRY_DELAY.min(remaining));
    }
    true
}

fn terminal_for_cancel(
    control: &OperationControl,
    results: &[CandidateResult],
) -> Option<ProfileWorkerResult> {
    match control.cancel_reason() {
        CancelReason::None => None,
        CancelReason::User => Some(ProfileWorkerResult::Cancelled(results.to_vec())),
        CancelReason::OperationDeadline => {
            Some(ProfileWorkerResult::OperationTimedOut(results.to_vec()))
        }
    }
}

fn cancelled_result(
    ordinal: usize,
    tuple: ModeTuple,
    attempt_count: u8,
    retry_reason: Option<CandidateRetryReason>,
) -> CandidateResult {
    CandidateResult::failed(
        ordinal,
        tuple,
        attempt_count,
        retry_reason,
        CaptureModeStatus::Cancelled,
        vec![CandidateFailureReason::Cancelled],
    )
}

fn timed_out_result(ordinal: usize, tuple: ModeTuple) -> CandidateResult {
    CandidateResult::failed(
        ordinal,
        tuple,
        1,
        None,
        CaptureModeStatus::CandidateTimedOut,
        vec![CandidateFailureReason::CandidateDeadline],
    )
}

fn status_for_retry(reason: CandidateRetryReason) -> CaptureModeStatus {
    match reason {
        CandidateRetryReason::OpenFailed => CaptureModeStatus::OpeningFailed,
        CandidateRetryReason::FirstReadStartFailed => CaptureModeStatus::FirstFrameTimeout,
    }
}

fn failure_for_retry(reason: CandidateRetryReason) -> CandidateFailureReason {
    match reason {
        CandidateRetryReason::OpenFailed => CandidateFailureReason::OpenFailed,
        CandidateRetryReason::FirstReadStartFailed => CandidateFailureReason::FirstFrameTimeout,
    }
}
