use std::{
    sync::{
        Arc, Condvar, Mutex, MutexGuard,
        atomic::{AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use crate::camera::{
    CameraFailureCode, CameraServiceError, CameraServiceResult, CameraServiceSnapshot,
    CameraServiceState, CaptureAdapterFactory, DeviceEndpoint, DeviceScanPolicy,
    DeviceScanSnapshot, DeviceScanStatus, MonotonicClock, OperationId, ProbeOutcome, ProbeTarget,
    SystemMonotonicClock,
    worker::{CancelReason, OperationControl, WorkerContext, WorkerResult, run_worker},
};

static SERVICE_INSTANCE_SEQUENCE: AtomicU64 = AtomicU64::new(1);
const WATCHDOG_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Clone)]
pub struct CameraService {
    core: Arc<ServiceCore>,
    adapter_factory: Arc<dyn CaptureAdapterFactory>,
    clock: Arc<dyn MonotonicClock>,
    service_instance: u64,
}

impl CameraService {
    pub fn new(adapter_factory: Arc<dyn CaptureAdapterFactory>) -> Self {
        Self::with_clock(adapter_factory, Arc::new(SystemMonotonicClock::new()))
    }

    pub fn with_clock(
        adapter_factory: Arc<dyn CaptureAdapterFactory>,
        clock: Arc<dyn MonotonicClock>,
    ) -> Self {
        Self {
            core: Arc::new(ServiceCore::new()),
            adapter_factory,
            clock,
            service_instance: SERVICE_INSTANCE_SEQUENCE.fetch_add(1, Ordering::Relaxed),
        }
    }

    pub fn start(&self, policy: DeviceScanPolicy) -> CameraServiceResult<DeviceScanSnapshot> {
        self.reap_finished_supervisor()?;
        let mut inner = self.core.lock()?;
        match inner.state {
            CameraServiceState::Idle => {}
            CameraServiceState::Scanning => return Err(CameraServiceError::Busy),
            CameraServiceState::Faulted => return Err(CameraServiceError::Busy),
            CameraServiceState::Stuck => return Err(CameraServiceError::Stuck),
        }
        let generation = inner
            .generation
            .checked_add(1)
            .ok_or(CameraServiceError::GenerationExhausted)?;
        let operation_id = OperationId::new(self.service_instance, generation);
        let control = Arc::new(OperationControl::new());
        inner.state = CameraServiceState::Scanning;
        inner.operation = Some(OperationRecord::new(
            operation_id.clone(),
            generation,
            policy.clone(),
            self.clock.now(),
        ));
        inner.control = Some(Arc::clone(&control));
        drop(inner);

        let core = Arc::clone(&self.core);
        let adapter_factory = Arc::clone(&self.adapter_factory);
        let clock = Arc::clone(&self.clock);
        let service_instance = self.service_instance;
        let supervisor = thread::Builder::new()
            .name(format!("camera-watchdog-{generation}"))
            .spawn(move || {
                supervise(
                    core,
                    adapter_factory,
                    clock,
                    control,
                    policy,
                    service_instance,
                    generation,
                );
            })
            .map_err(|source| {
                if let Ok(mut failed) = self.core.inner.lock() {
                    failed.state = CameraServiceState::Idle;
                    failed.operation = None;
                    failed.control = None;
                }
                CameraServiceError::WorkerStart { source }
            })?;

        let mut inner = self.core.lock()?;
        inner.generation = generation;
        inner.supervisor = Some(supervisor);
        let snapshot = inner
            .operation
            .as_ref()
            .map(|operation| operation.snapshot(inner.state))
            .ok_or(CameraServiceError::Synchronization)?;
        Ok(snapshot)
    }

    pub fn get(&self, operation_id: &str) -> CameraServiceResult<DeviceScanSnapshot> {
        let inner = self.core.lock()?;
        let operation = inner
            .operation
            .as_ref()
            .filter(|operation| operation.operation_id.as_str() == operation_id)
            .ok_or(CameraServiceError::StaleOperation)?;
        Ok(operation.snapshot(inner.state))
    }

    pub fn cancel(&self, operation_id: &str) -> CameraServiceResult<DeviceScanSnapshot> {
        let wait_deadline = {
            let mut inner = self.core.lock()?;
            let is_requested_operation = inner
                .operation
                .as_ref()
                .is_some_and(|operation| operation.operation_id.as_str() == operation_id);
            if !is_requested_operation {
                return Err(CameraServiceError::StaleOperation);
            }
            if inner
                .operation
                .as_ref()
                .is_some_and(|operation| operation.status.is_terminal())
            {
                let operation = inner
                    .operation
                    .as_ref()
                    .ok_or(CameraServiceError::Synchronization)?;
                return Ok(operation.snapshot(inner.state));
            }
            let shutdown_deadline = inner
                .operation
                .as_ref()
                .map(|operation| operation.policy.shutdown_deadline())
                .ok_or(CameraServiceError::Synchronization)?;
            if let Some(control) = inner.control.as_ref() {
                control.request_user_cancel();
            }
            if let Some(operation) = inner.operation.as_mut() {
                operation
                    .cancel_requested_at
                    .get_or_insert(self.clock.now());
            }
            self.core.changed.notify_all();
            shutdown_deadline.saturating_add(Duration::from_secs(1))
        };
        self.wait_for_terminal(operation_id, wait_deadline)
    }

    pub fn stop(&self) -> CameraServiceResult<CameraServiceSnapshot> {
        let operation_id = {
            let mut inner = self.core.lock()?;
            match inner.state {
                CameraServiceState::Idle => {
                    drop(inner);
                    self.reap_finished_supervisor()?;
                    return self.snapshot();
                }
                CameraServiceState::Faulted => {
                    inner.state = CameraServiceState::Idle;
                    if let Some(operation) = inner.operation.as_mut() {
                        operation.service_state = CameraServiceState::Idle;
                    }
                    drop(inner);
                    self.reap_finished_supervisor()?;
                    return self.snapshot();
                }
                CameraServiceState::Stuck => return Ok(inner.snapshot()),
                CameraServiceState::Scanning => {}
            }
            inner
                .operation
                .as_ref()
                .map(|operation| operation.operation_id.as_str().to_owned())
                .ok_or(CameraServiceError::Synchronization)?
        };
        let _ = self.cancel(&operation_id)?;
        self.snapshot()
    }

    pub fn snapshot(&self) -> CameraServiceResult<CameraServiceSnapshot> {
        let inner = self.core.lock()?;
        Ok(inner.snapshot())
    }

    fn wait_for_terminal(
        &self,
        operation_id: &str,
        maximum_wait: Duration,
    ) -> CameraServiceResult<DeviceScanSnapshot> {
        let started_at = std::time::Instant::now();
        let mut inner = self.core.lock()?;
        loop {
            let operation = inner
                .operation
                .as_ref()
                .filter(|operation| operation.operation_id.as_str() == operation_id)
                .ok_or(CameraServiceError::StaleOperation)?;
            if operation.status.is_terminal() {
                return Ok(operation.snapshot(inner.state));
            }
            let remaining = maximum_wait.saturating_sub(started_at.elapsed());
            if remaining.is_zero() {
                return Err(CameraServiceError::Synchronization);
            }
            let waited = self
                .core
                .changed
                .wait_timeout(inner, WATCHDOG_INTERVAL.min(remaining))
                .map_err(|_| CameraServiceError::Synchronization)?;
            inner = waited.0;
        }
    }

    fn reap_finished_supervisor(&self) -> CameraServiceResult<()> {
        let handle = {
            let mut inner = self.core.lock()?;
            if inner
                .supervisor
                .as_ref()
                .is_some_and(JoinHandle::is_finished)
            {
                inner.supervisor.take()
            } else {
                None
            }
        };
        if let Some(handle) = handle {
            handle
                .join()
                .map_err(|_| CameraServiceError::WorkerPanicked)?;
        }
        Ok(())
    }
}

pub(crate) struct ServiceCore {
    inner: Mutex<ServiceInner>,
    changed: Condvar,
}

impl ServiceCore {
    fn new() -> Self {
        Self {
            inner: Mutex::new(ServiceInner::new()),
            changed: Condvar::new(),
        }
    }

    fn lock(&self) -> CameraServiceResult<MutexGuard<'_, ServiceInner>> {
        self.inner
            .lock()
            .map_err(|_| CameraServiceError::Synchronization)
    }

    pub(crate) fn begin_probe(&self, target: ProbeTarget, epoch: u64, now: Duration) {
        if let Ok(mut inner) = self.inner.lock() {
            if let Some(operation) = inner.operation.as_mut() {
                operation.current_probe = Some(target);
                operation.current_probe_epoch = epoch;
                operation.current_probe_started_at = Some(now);
                operation.probe_timeout_requested = None;
                operation.last_progress_at = now;
            }
            self.changed.notify_all();
        }
    }

    pub(crate) fn note_progress(&self, now: Duration) {
        if let Ok(mut inner) = self.inner.lock() {
            if let Some(operation) = inner.operation.as_mut() {
                operation.last_progress_at = now;
            }
            self.changed.notify_all();
        }
    }

    pub(crate) fn complete_probe(
        &self,
        outcome: ProbeOutcome,
        endpoint: Option<DeviceEndpoint>,
        now: Duration,
    ) {
        if let Ok(mut inner) = self.inner.lock() {
            if let Some(operation) = inner.operation.as_mut() {
                operation.outcomes.push(outcome);
                if let Some(endpoint) = endpoint {
                    operation.endpoints.push(endpoint);
                }
                operation.completed_probes = operation.outcomes.len();
                operation.current_probe = None;
                operation.current_probe_started_at = None;
                operation.probe_timeout_requested = None;
                operation.last_progress_at = now;
            }
            self.changed.notify_all();
        }
    }
}

struct ServiceInner {
    state: CameraServiceState,
    generation: u64,
    operation: Option<OperationRecord>,
    control: Option<Arc<OperationControl>>,
    supervisor: Option<JoinHandle<()>>,
}

impl ServiceInner {
    fn new() -> Self {
        Self {
            state: CameraServiceState::Idle,
            generation: 0,
            operation: None,
            control: None,
            supervisor: None,
        }
    }

    fn snapshot(&self) -> CameraServiceSnapshot {
        CameraServiceSnapshot::new(
            self.state,
            self.operation
                .as_ref()
                .map(|operation| operation.snapshot(self.state)),
        )
    }
}

struct OperationRecord {
    operation_id: OperationId,
    generation: u64,
    policy: DeviceScanPolicy,
    service_state: CameraServiceState,
    status: DeviceScanStatus,
    completed_probes: usize,
    current_probe: Option<ProbeTarget>,
    current_probe_epoch: u64,
    outcomes: Vec<ProbeOutcome>,
    endpoints: Vec<DeviceEndpoint>,
    failure_code: Option<CameraFailureCode>,
    started_at: Duration,
    current_probe_started_at: Option<Duration>,
    probe_timeout_requested: Option<(u64, Duration)>,
    cancel_requested_at: Option<Duration>,
    last_progress_at: Duration,
}

impl OperationRecord {
    fn new(
        operation_id: OperationId,
        generation: u64,
        policy: DeviceScanPolicy,
        started_at: Duration,
    ) -> Self {
        Self {
            operation_id,
            generation,
            policy,
            service_state: CameraServiceState::Scanning,
            status: DeviceScanStatus::Scanning,
            completed_probes: 0,
            current_probe: None,
            current_probe_epoch: 0,
            outcomes: Vec::new(),
            endpoints: Vec::new(),
            failure_code: None,
            started_at,
            current_probe_started_at: None,
            probe_timeout_requested: None,
            cancel_requested_at: None,
            last_progress_at: started_at,
        }
    }

    fn snapshot(&self, service_state: CameraServiceState) -> DeviceScanSnapshot {
        DeviceScanSnapshot {
            operation_id: self.operation_id.clone(),
            scan_generation: self.generation,
            policy: self.policy.clone(),
            service_state,
            status: self.status,
            completed_probes: self.completed_probes.min(self.policy.total_probes()),
            total_probes: self.policy.total_probes(),
            current_probe: self.current_probe,
            outcomes: self.outcomes.clone(),
            endpoints: self.endpoints.clone(),
            failure_code: self.failure_code,
        }
    }
}

fn supervise(
    core: Arc<ServiceCore>,
    adapter_factory: Arc<dyn CaptureAdapterFactory>,
    clock: Arc<dyn MonotonicClock>,
    control: Arc<OperationControl>,
    policy: DeviceScanPolicy,
    service_instance: u64,
    generation: u64,
) {
    let worker_context = WorkerContext {
        core: Arc::clone(&core),
        control: Arc::clone(&control),
        clock: Arc::clone(&clock),
        policy: policy.clone(),
        service_instance,
        generation,
    };
    let worker = thread::Builder::new()
        .name(format!("camera-worker-{generation}"))
        .spawn(move || run_worker(adapter_factory.create(), worker_context));
    let Ok(worker) = worker else {
        set_terminal(
            &core,
            CameraServiceState::Faulted,
            DeviceScanStatus::Failed,
            Some(CameraFailureCode::Internal),
        );
        return;
    };

    let mut stuck = false;
    while !worker.is_finished() {
        let now = clock.now();
        let mut inner = match core.inner.lock() {
            Ok(inner) => inner,
            Err(_) => {
                control.request_user_cancel();
                break;
            }
        };
        let Some(operation) = inner.operation.as_mut() else {
            break;
        };

        if control.cancel_reason() == CancelReason::None
            && now.saturating_sub(operation.started_at) >= policy.operation_deadline()
        {
            control.request_operation_timeout();
            operation.cancel_requested_at.get_or_insert(now);
        }

        if let Some(probe_started_at) = operation.current_probe_started_at {
            let epoch = operation.current_probe_epoch;
            if operation.probe_timeout_requested.is_none()
                && now.saturating_sub(probe_started_at) >= policy.first_frame_deadline()
            {
                control.request_probe_timeout(epoch);
                operation.probe_timeout_requested = Some((epoch, now));
            }
        }

        let cancellation_stalled = operation.cancel_requested_at.is_some_and(|requested_at| {
            now.saturating_sub(requested_at) >= policy.shutdown_deadline()
        });
        let probe_stalled =
            operation
                .probe_timeout_requested
                .is_some_and(|(epoch, requested_at)| {
                    epoch == operation.current_probe_epoch
                        && operation.current_probe.is_some()
                        && now.saturating_sub(requested_at) >= policy.shutdown_deadline()
                });
        if cancellation_stalled || probe_stalled {
            operation.service_state = CameraServiceState::Stuck;
            operation.status = DeviceScanStatus::Stuck;
            operation.failure_code = Some(CameraFailureCode::ReadStalled);
            inner.state = CameraServiceState::Stuck;
            stuck = true;
            core.changed.notify_all();
            drop(inner);
            break;
        }
        let waited = core.changed.wait_timeout(inner, WATCHDOG_INTERVAL);
        if waited.is_err() {
            control.request_user_cancel();
            break;
        }
    }

    let joined = worker.join();
    if stuck {
        return;
    }
    match joined {
        Ok(WorkerResult::Completed) => set_terminal(
            &core,
            CameraServiceState::Idle,
            DeviceScanStatus::Completed,
            None,
        ),
        Ok(WorkerResult::Cancelled) => set_terminal(
            &core,
            CameraServiceState::Idle,
            DeviceScanStatus::Cancelled,
            Some(CameraFailureCode::Cancelled),
        ),
        Ok(WorkerResult::OperationTimedOut) => set_terminal(
            &core,
            CameraServiceState::Faulted,
            DeviceScanStatus::Failed,
            Some(CameraFailureCode::ReadTimeout),
        ),
        Ok(WorkerResult::ReleaseFailed(error)) => {
            let _ = error;
            set_terminal(
                &core,
                CameraServiceState::Stuck,
                DeviceScanStatus::Stuck,
                Some(CameraFailureCode::Internal),
            );
        }
        Err(_) => set_terminal(
            &core,
            CameraServiceState::Stuck,
            DeviceScanStatus::Stuck,
            Some(CameraFailureCode::Internal),
        ),
    }
}

fn set_terminal(
    core: &ServiceCore,
    service_state: CameraServiceState,
    status: DeviceScanStatus,
    failure_code: Option<CameraFailureCode>,
) {
    if let Ok(mut inner) = core.inner.lock() {
        inner.state = service_state;
        inner.control = None;
        if let Some(operation) = inner.operation.as_mut() {
            operation.service_state = service_state;
            operation.status = status;
            operation.current_probe = None;
            operation.current_probe_started_at = None;
            operation.failure_code = failure_code;
        }
        core.changed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        io,
        sync::{
            Arc, Condvar, Mutex,
            atomic::{AtomicU64, Ordering},
        },
        thread,
        time::{Duration, Instant},
    };

    use super::*;
    use crate::camera::{
        CaptureAdapter, CaptureAdapterError, CaptureAdapterResult, CaptureBackend, CaptureOpen,
        CaptureSession, FrameRead, ProbeStatus,
    };

    #[derive(Clone)]
    enum OpenScript {
        Unavailable,
        OpenError,
        Session {
            reads: Vec<Result<FrameRead, ()>>,
            release_error: bool,
        },
        Blocking {
            gate: Arc<(Mutex<bool>, Condvar)>,
        },
    }

    struct ScriptState {
        scripts: Mutex<VecDeque<OpenScript>>,
        targets: Mutex<Vec<ProbeTarget>>,
        active: Mutex<usize>,
        maximum_active: Mutex<usize>,
        releases: Mutex<usize>,
    }

    struct ManualClock {
        milliseconds: AtomicU64,
    }

    impl ManualClock {
        fn new() -> Self {
            Self {
                milliseconds: AtomicU64::new(0),
            }
        }

        fn advance(&self, milliseconds: u64) {
            self.milliseconds.fetch_add(milliseconds, Ordering::SeqCst);
        }
    }

    impl MonotonicClock for ManualClock {
        fn now(&self) -> Duration {
            Duration::from_millis(self.milliseconds.load(Ordering::SeqCst))
        }

        fn sleep(&self, _duration: Duration) {
            thread::sleep(Duration::from_millis(1));
        }
    }

    impl ScriptState {
        fn new(scripts: impl IntoIterator<Item = OpenScript>) -> Arc<Self> {
            Arc::new(Self {
                scripts: Mutex::new(scripts.into_iter().collect()),
                targets: Mutex::new(Vec::new()),
                active: Mutex::new(0),
                maximum_active: Mutex::new(0),
                releases: Mutex::new(0),
            })
        }
    }

    struct ScriptFactory(Arc<ScriptState>);

    impl CaptureAdapterFactory for ScriptFactory {
        fn create(&self) -> Box<dyn CaptureAdapter> {
            Box::new(ScriptAdapter(Arc::clone(&self.0)))
        }
    }

    struct ScriptAdapter(Arc<ScriptState>);

    impl CaptureAdapter for ScriptAdapter {
        fn open(&mut self, target: ProbeTarget) -> CaptureAdapterResult<CaptureOpen> {
            self.0
                .targets
                .lock()
                .map_err(|_| marker_open())?
                .push(target);
            let script = self
                .0
                .scripts
                .lock()
                .map_err(|_| marker_open())?
                .pop_front()
                .unwrap_or(OpenScript::Unavailable);
            match script {
                OpenScript::Unavailable => Ok(CaptureOpen::Unavailable),
                OpenScript::OpenError => Err(marker_open()),
                OpenScript::Session {
                    reads,
                    release_error,
                } => {
                    activate(&self.0)?;
                    Ok(CaptureOpen::Session(Box::new(ScriptSession {
                        state: Arc::clone(&self.0),
                        reads: reads.into(),
                        release_error,
                        released: false,
                    })))
                }
                OpenScript::Blocking { gate } => {
                    activate(&self.0)?;
                    Ok(CaptureOpen::Session(Box::new(BlockingSession {
                        state: Arc::clone(&self.0),
                        gate,
                        released: false,
                    })))
                }
            }
        }
    }

    fn activate(state: &ScriptState) -> CaptureAdapterResult<()> {
        let mut active = state.active.lock().map_err(|_| marker_open())?;
        *active += 1;
        let mut maximum = state.maximum_active.lock().map_err(|_| marker_open())?;
        *maximum = (*maximum).max(*active);
        Ok(())
    }

    struct ScriptSession {
        state: Arc<ScriptState>,
        reads: VecDeque<Result<FrameRead, ()>>,
        release_error: bool,
        released: bool,
    }

    impl CaptureSession for ScriptSession {
        fn read(&mut self) -> CaptureAdapterResult<FrameRead> {
            match self.reads.pop_front().unwrap_or(Ok(FrameRead::Empty)) {
                Ok(value) => Ok(value),
                Err(()) => Err(CaptureAdapterError::read(io::Error::other("READ_MARKER"))),
            }
        }

        fn release(&mut self) -> CaptureAdapterResult<()> {
            release_script_session(&self.state, &mut self.released)?;
            if self.release_error {
                Err(CaptureAdapterError::release(io::Error::other(
                    "RELEASE_MARKER",
                )))
            } else {
                Ok(())
            }
        }
    }

    impl Drop for ScriptSession {
        fn drop(&mut self) {
            let _ = release_script_session(&self.state, &mut self.released);
        }
    }

    struct BlockingSession {
        state: Arc<ScriptState>,
        gate: Arc<(Mutex<bool>, Condvar)>,
        released: bool,
    }

    impl CaptureSession for BlockingSession {
        fn read(&mut self) -> CaptureAdapterResult<FrameRead> {
            let (lock, changed) = &*self.gate;
            let mut ready = lock.lock().map_err(|_| marker_read())?;
            while !*ready {
                ready = changed.wait(ready).map_err(|_| marker_read())?;
            }
            Ok(FrameRead::Frame)
        }

        fn release(&mut self) -> CaptureAdapterResult<()> {
            release_script_session(&self.state, &mut self.released)
        }
    }

    impl Drop for BlockingSession {
        fn drop(&mut self) {
            let _ = release_script_session(&self.state, &mut self.released);
        }
    }

    fn release_script_session(
        state: &ScriptState,
        released: &mut bool,
    ) -> CaptureAdapterResult<()> {
        if *released {
            return Ok(());
        }
        *released = true;
        let mut active = state.active.lock().map_err(|_| marker_release())?;
        *active = active.saturating_sub(1);
        *state.releases.lock().map_err(|_| marker_release())? += 1;
        Ok(())
    }

    fn marker_open() -> CaptureAdapterError {
        CaptureAdapterError::open(io::Error::other("OPEN_MARKER"))
    }

    fn marker_read() -> CaptureAdapterError {
        CaptureAdapterError::read(io::Error::other("READ_MARKER"))
    }

    fn marker_release() -> CaptureAdapterError {
        CaptureAdapterError::release(io::Error::other("RELEASE_MARKER"))
    }

    fn policy(backends: Vec<CaptureBackend>, last_index: u32) -> DeviceScanPolicy {
        DeviceScanPolicy::new(0, last_index, backends, 50, 2_000, 100, 1)
            .expect("test policy is valid")
    }

    fn wait_for_terminal(service: &CameraService, operation_id: &str) -> DeviceScanSnapshot {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let snapshot = service.get(operation_id).expect("snapshot is available");
            if snapshot.status().is_terminal() {
                return snapshot;
            }
            assert!(Instant::now() < deadline, "camera worker did not finish");
            thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn scripted_adapter_covers_open_empty_read_first_frame_and_release() {
        let state = ScriptState::new([
            OpenScript::OpenError,
            OpenScript::Session {
                reads: vec![Ok(FrameRead::Empty), Err(())],
                release_error: false,
            },
            OpenScript::Session {
                reads: vec![Ok(FrameRead::Frame)],
                release_error: false,
            },
        ]);
        let service = CameraService::new(Arc::new(ScriptFactory(Arc::clone(&state))));
        let started = service
            .start(policy(vec![CaptureBackend::Msmf], 2))
            .expect("scan starts");
        let snapshot = wait_for_terminal(&service, started.operation_id().as_str());

        assert_eq!(snapshot.status(), DeviceScanStatus::Completed);
        assert_eq!(
            (snapshot.completed_probes(), snapshot.total_probes()),
            (3, 3)
        );
        assert_eq!(
            snapshot
                .outcomes()
                .iter()
                .map(|outcome| outcome.status())
                .collect::<Vec<_>>(),
            vec![
                ProbeStatus::OpenFailed,
                ProbeStatus::ReadFailed,
                ProbeStatus::Available
            ]
        );
        assert_eq!(snapshot.endpoints().len(), 1);
        assert_eq!(*state.releases.lock().expect("release count"), 2);
        assert_eq!(*state.maximum_active.lock().expect("max active"), 1);
    }

    #[test]
    fn empty_frames_timeout_without_aborting_remaining_probes() {
        let state = ScriptState::new([
            OpenScript::Session {
                reads: Vec::new(),
                release_error: false,
            },
            OpenScript::Unavailable,
        ]);
        let service = CameraService::new(Arc::new(ScriptFactory(Arc::clone(&state))));
        let started = service
            .start(policy(vec![CaptureBackend::Msmf], 1))
            .expect("scan starts");
        let snapshot = wait_for_terminal(&service, started.operation_id().as_str());

        assert_eq!(snapshot.status(), DeviceScanStatus::Completed);
        assert_eq!(
            snapshot
                .outcomes()
                .iter()
                .map(|outcome| outcome.status())
                .collect::<Vec<_>>(),
            vec![ProbeStatus::FirstFrameTimeout, ProbeStatus::OpenFailed]
        );
        assert!(snapshot.endpoints().is_empty());
        assert_eq!(*state.maximum_active.lock().expect("max active"), 1);
    }

    #[test]
    fn no_camera_is_a_successful_empty_result() {
        let state = ScriptState::new([OpenScript::Unavailable, OpenScript::OpenError]);
        let service = CameraService::new(Arc::new(ScriptFactory(state)));
        let started = service
            .start(policy(vec![CaptureBackend::Msmf], 1))
            .expect("scan starts");
        let snapshot = wait_for_terminal(&service, started.operation_id().as_str());
        assert_eq!(snapshot.status(), DeviceScanStatus::Completed);
        assert_eq!(snapshot.completed_probes(), 2);
        assert!(snapshot.endpoints().is_empty());
    }

    #[test]
    fn scan_is_ordered_and_same_index_backends_have_distinct_identity() {
        let state = ScriptState::new([
            OpenScript::Session {
                reads: vec![Ok(FrameRead::Frame)],
                release_error: false,
            },
            OpenScript::Session {
                reads: vec![Ok(FrameRead::Frame)],
                release_error: false,
            },
        ]);
        let service = CameraService::new(Arc::new(ScriptFactory(Arc::clone(&state))));
        let started = service
            .start(policy(vec![CaptureBackend::Msmf, CaptureBackend::Dshow], 0))
            .expect("scan starts");
        let snapshot = wait_for_terminal(&service, started.operation_id().as_str());

        assert_eq!(snapshot.endpoints().len(), 2);
        assert_ne!(snapshot.endpoints()[0].key(), snapshot.endpoints()[1].key());
        assert_eq!(
            *state.targets.lock().expect("target order"),
            vec![
                ProbeTarget::new(CaptureBackend::Msmf, 0),
                ProbeTarget::new(CaptureBackend::Dshow, 0)
            ]
        );
    }

    #[test]
    fn generation_increments_only_for_accepted_start_and_ids_are_process_local() {
        let first_state = ScriptState::new([OpenScript::Unavailable, OpenScript::Unavailable]);
        let service = CameraService::new(Arc::new(ScriptFactory(first_state)));
        let first = service
            .start(policy(vec![CaptureBackend::Msmf], 0))
            .expect("first scan starts");
        wait_for_terminal(&service, first.operation_id().as_str());
        let second = service
            .start(policy(vec![CaptureBackend::Msmf], 0))
            .expect("second scan starts");
        wait_for_terminal(&service, second.operation_id().as_str());
        assert_eq!(first.scan_generation(), 1);
        assert_eq!(second.scan_generation(), 2);
        assert_ne!(first.operation_id(), second.operation_id());

        let other = CameraService::new(Arc::new(ScriptFactory(ScriptState::new([
            OpenScript::Unavailable,
        ]))));
        let other_scan = other
            .start(policy(vec![CaptureBackend::Msmf], 0))
            .expect("other service starts");
        assert_eq!(other_scan.scan_generation(), 1);
        assert_ne!(first.operation_id(), other_scan.operation_id());

        let mut inner = service.core.inner.lock().expect("service lock");
        inner.generation = u64::MAX;
        drop(inner);
        assert!(matches!(
            service.start(policy(vec![CaptureBackend::Msmf], 0)),
            Err(CameraServiceError::GenerationExhausted)
        ));
    }

    #[test]
    fn concurrent_start_is_busy_and_cancel_is_idempotent() {
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let state = ScriptState::new([OpenScript::Blocking {
            gate: Arc::clone(&gate),
        }]);
        let service = CameraService::new(Arc::new(ScriptFactory(state)));
        let started = service
            .start(policy(vec![CaptureBackend::Msmf], 0))
            .expect("scan starts");
        assert!(matches!(
            service.start(policy(vec![CaptureBackend::Msmf], 0)),
            Err(CameraServiceError::Busy)
        ));
        let probe_deadline = Instant::now() + Duration::from_secs(1);
        while service
            .get(started.operation_id().as_str())
            .expect("snapshot")
            .current_probe()
            .is_none()
        {
            assert!(Instant::now() < probe_deadline, "probe did not begin");
            thread::sleep(Duration::from_millis(1));
        }
        let service_for_cancel = service.clone();
        let operation_id = started.operation_id().as_str().to_owned();
        let cancellation = thread::spawn(move || service_for_cancel.cancel(&operation_id));
        thread::sleep(Duration::from_millis(10));
        let (lock, changed) = &*gate;
        *lock.lock().expect("gate lock") = true;
        changed.notify_all();
        let cancelled = cancellation
            .join()
            .expect("cancel thread joins")
            .expect("cancel succeeds");
        assert_eq!(cancelled.status(), DeviceScanStatus::Cancelled);
        assert!(cancelled.endpoints().is_empty());
        assert_eq!(
            cancelled.outcomes().last().map(|outcome| outcome.status()),
            Some(ProbeStatus::Cancelled)
        );
        assert_eq!(
            service
                .cancel(started.operation_id().as_str())
                .expect("repeat cancel is idempotent")
                .status(),
            DeviceScanStatus::Cancelled
        );
        assert_eq!(
            service.stop().expect("idle stop succeeds").service_state(),
            CameraServiceState::Idle
        );
    }

    #[test]
    fn cancellation_between_probes_prevents_the_next_open() {
        let state = ScriptState::new([
            OpenScript::Session {
                reads: vec![Ok(FrameRead::Frame)],
                release_error: false,
            },
            OpenScript::Session {
                reads: vec![Ok(FrameRead::Frame)],
                release_error: false,
            },
        ]);
        let service = CameraService::new(Arc::new(ScriptFactory(Arc::clone(&state))));
        let policy = DeviceScanPolicy::new(0, 1, vec![CaptureBackend::Msmf], 20, 1_000, 100, 200)
            .expect("cancellation policy is valid");
        let started = service.start(policy).expect("scan starts");
        let deadline = Instant::now() + Duration::from_secs(1);
        while *state.releases.lock().expect("release count") == 0 {
            assert!(Instant::now() < deadline, "first probe did not release");
            thread::sleep(Duration::from_millis(1));
        }

        let cancelled = service
            .cancel(started.operation_id().as_str())
            .expect("cancel completes");
        assert_eq!(cancelled.status(), DeviceScanStatus::Cancelled);
        assert_eq!(cancelled.completed_probes(), 1);
        assert_eq!(state.targets.lock().expect("target list").len(), 1);
    }

    #[test]
    fn operation_timeout_after_cleanup_enters_faulted_and_stop_resets_idle() {
        let state = ScriptState::new([OpenScript::Unavailable, OpenScript::Unavailable]);
        let service = CameraService::new(Arc::new(ScriptFactory(state)));
        let policy = DeviceScanPolicy::new(0, 1, vec![CaptureBackend::Msmf], 10, 30, 10, 50)
            .expect("timeout policy is valid");
        let started = service.start(policy).expect("scan starts");
        let failed = wait_for_terminal(&service, started.operation_id().as_str());
        assert_eq!(failed.status(), DeviceScanStatus::Failed);
        assert_eq!(failed.service_state(), CameraServiceState::Faulted);
        assert_eq!(failed.failure_code(), Some(CameraFailureCode::ReadTimeout));
        assert_eq!(
            service
                .stop()
                .expect("fault reset succeeds")
                .service_state(),
            CameraServiceState::Idle
        );
        assert!(
            service
                .core
                .inner
                .lock()
                .expect("service lock")
                .supervisor
                .is_none(),
            "completed supervisor must be joined"
        );
    }

    #[test]
    fn blocked_native_call_becomes_sticky_stuck_and_late_return_does_not_restore_idle() {
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let state = ScriptState::new([OpenScript::Blocking {
            gate: Arc::clone(&gate),
        }]);
        let clock = Arc::new(ManualClock::new());
        let service = CameraService::with_clock(
            Arc::new(ScriptFactory(state)),
            Arc::clone(&clock) as Arc<dyn MonotonicClock>,
        );
        let policy = DeviceScanPolicy::new(0, 0, vec![CaptureBackend::Msmf], 10, 500, 10, 1)
            .expect("stuck policy is valid");
        let started = service.start(policy).expect("scan starts");
        let probe_deadline = Instant::now() + Duration::from_secs(1);
        while service
            .get(started.operation_id().as_str())
            .expect("snapshot")
            .current_probe()
            .is_none()
        {
            assert!(Instant::now() < probe_deadline, "probe did not begin");
            thread::sleep(Duration::from_millis(1));
        }
        clock.advance(11);
        let timeout_deadline = Instant::now() + Duration::from_secs(1);
        loop {
            let requested = service
                .core
                .inner
                .lock()
                .expect("service lock")
                .operation
                .as_ref()
                .is_some_and(|operation| operation.probe_timeout_requested.is_some());
            if requested {
                break;
            }
            assert!(
                Instant::now() < timeout_deadline,
                "watchdog did not observe fake probe deadline"
            );
            thread::sleep(Duration::from_millis(1));
        }
        clock.advance(11);
        let stuck = wait_for_terminal(&service, started.operation_id().as_str());
        assert_eq!(stuck.status(), DeviceScanStatus::Stuck);
        assert_eq!(stuck.service_state(), CameraServiceState::Stuck);
        assert!(matches!(
            service.start(DeviceScanPolicy::defaults()),
            Err(CameraServiceError::Stuck)
        ));
        assert!(
            service
                .core
                .inner
                .lock()
                .expect("service lock")
                .supervisor
                .is_some(),
            "stuck worker join ownership must remain with the service"
        );

        let (lock, changed) = &*gate;
        *lock.lock().expect("gate lock") = true;
        changed.notify_all();
        thread::sleep(Duration::from_millis(20));
        assert_eq!(
            service.snapshot().expect("snapshot").service_state(),
            CameraServiceState::Stuck
        );
        assert_eq!(
            service
                .stop()
                .expect("stuck stop reports state")
                .service_state(),
            CameraServiceState::Stuck
        );
    }

    #[test]
    fn release_failure_never_claims_successful_cleanup() {
        let state = ScriptState::new([OpenScript::Session {
            reads: vec![Ok(FrameRead::Frame)],
            release_error: true,
        }]);
        let service = CameraService::new(Arc::new(ScriptFactory(state)));
        let started = service
            .start(policy(vec![CaptureBackend::Msmf], 0))
            .expect("scan starts");
        let snapshot = wait_for_terminal(&service, started.operation_id().as_str());
        assert_eq!(snapshot.status(), DeviceScanStatus::Stuck);
        assert_ne!(snapshot.service_state(), CameraServiceState::Idle);
    }
}
