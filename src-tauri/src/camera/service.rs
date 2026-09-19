use std::{
    sync::{
        Arc, Condvar, Mutex, MutexGuard,
        atomic::{AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::camera::{
    CameraFailureCode, CameraServiceError, CameraServiceResult, CameraServiceSnapshot,
    CameraServiceState, CaptureAdapterFactory, DeviceEndpoint, DeviceScanPolicy,
    DeviceScanSnapshot, DeviceScanStatus, MonotonicClock, OperationId, ProbeOutcome, ProbeTarget,
    SystemMonotonicClock,
    profile_worker::{ProfileWorkerContext, ProfileWorkerResult, run_profile_worker},
    profiling::{
        CandidateFailureReason, CandidatePhase, CandidateResult, EnvironmentVersionReferences,
        ModeCandidatePolicy, ModeCandidatePolicyV1, ModeTuple, ProfileId, ProfileOperationStatus,
        ProfileReport, ProfileReportInput, ProfileSnapshot, ProfileTerminalStatus,
        VerifiedModeDescriptor, VerifiedModeRegistry,
    },
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
            CameraServiceState::Idle | CameraServiceState::ProfileReady => {}
            CameraServiceState::Scanning | CameraServiceState::Profiling => {
                return Err(CameraServiceError::Busy);
            }
            CameraServiceState::Faulted => return Err(CameraServiceError::Busy),
            CameraServiceState::Stuck => return Err(CameraServiceError::Stuck),
        }
        let generation = inner
            .generation
            .checked_add(1)
            .ok_or(CameraServiceError::GenerationExhausted)?;
        let operation_id = OperationId::new(self.service_instance, generation);
        let control = Arc::new(OperationControl::new());
        inner.verified_modes.invalidate();
        inner.state = CameraServiceState::Scanning;
        inner.active_operation = Some(ActiveOperationKind::Scan);
        inner.last_scan = Some(OperationRecord::new(
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
                    failed.last_scan = None;
                    failed.active_operation = None;
                    failed.control = None;
                }
                CameraServiceError::WorkerStart { source }
            })?;

        let mut inner = self.core.lock()?;
        inner.generation = generation;
        inner.supervisor = Some(supervisor);
        let snapshot = inner
            .last_scan
            .as_ref()
            .map(|operation| operation.snapshot(inner.state))
            .ok_or(CameraServiceError::Synchronization)?;
        Ok(snapshot)
    }

    pub fn start_profile(
        &self,
        endpoint_key: &str,
        policy_wire: ModeCandidatePolicyV1,
    ) -> CameraServiceResult<ProfileSnapshot> {
        // Validation deliberately happens before state mutation or adapter creation.
        let policy = ModeCandidatePolicy::validate(policy_wire)?;
        let config_hash = policy.canonical_hash()?;
        self.reap_finished_supervisor()?;
        let mut inner = self.core.lock()?;
        match inner.state {
            CameraServiceState::Idle | CameraServiceState::ProfileReady => {}
            CameraServiceState::Scanning | CameraServiceState::Profiling => {
                return Err(CameraServiceError::Busy);
            }
            CameraServiceState::Faulted => return Err(CameraServiceError::Busy),
            CameraServiceState::Stuck => return Err(CameraServiceError::Stuck),
        }
        let endpoint = inner
            .last_scan
            .as_ref()
            .filter(|scan| scan.status == DeviceScanStatus::Completed)
            .and_then(|scan| {
                scan.endpoints
                    .iter()
                    .find(|endpoint| endpoint.key().as_str() == endpoint_key)
            })
            .cloned()
            .ok_or(CameraServiceError::StaleDeviceEndpoint)?;
        let generation = inner
            .profile_generation
            .checked_add(1)
            .ok_or(CameraServiceError::GenerationExhausted)?;
        let profile_id = ProfileId::issued(self.service_instance, generation);
        let control = Arc::new(OperationControl::new());
        inner.verified_modes.invalidate();
        inner.state = CameraServiceState::Profiling;
        inner.active_operation = Some(ActiveOperationKind::Profile);
        inner.last_profile = Some(ProfileRecord::new(
            profile_id,
            endpoint.clone(),
            policy.clone(),
            config_hash,
            self.clock.now(),
            unix_time_ms(),
        ));
        inner.control = Some(Arc::clone(&control));
        drop(inner);

        let core = Arc::clone(&self.core);
        let adapter_factory = Arc::clone(&self.adapter_factory);
        let clock = Arc::clone(&self.clock);
        let supervisor = thread::Builder::new()
            .name(format!("camera-profile-watchdog-{generation}"))
            .spawn(move || {
                supervise_profile(core, adapter_factory, clock, control, policy, endpoint);
            })
            .map_err(|source| {
                if let Ok(mut failed) = self.core.inner.lock() {
                    failed.state = CameraServiceState::Idle;
                    failed.active_operation = None;
                    failed.last_profile = None;
                    failed.control = None;
                }
                CameraServiceError::WorkerStart { source }
            })?;

        let mut inner = self.core.lock()?;
        inner.profile_generation = generation;
        inner.supervisor = Some(supervisor);
        inner
            .last_profile
            .as_ref()
            .map(|profile| profile.snapshot(inner.state))
            .ok_or(CameraServiceError::Synchronization)
    }

    pub fn get_profile_status(&self, profile_id: &str) -> CameraServiceResult<ProfileSnapshot> {
        let inner = self.core.lock()?;
        let profile = inner
            .last_profile
            .as_ref()
            .filter(|profile| profile.profile_id.as_str() == profile_id)
            .ok_or(CameraServiceError::StaleProfileOperation)?;
        Ok(profile.snapshot(inner.state))
    }

    pub fn get_profile_result(&self, profile_id: &str) -> CameraServiceResult<ProfileReport> {
        let inner = self.core.lock()?;
        let profile = inner
            .last_profile
            .as_ref()
            .filter(|profile| profile.profile_id.as_str() == profile_id)
            .ok_or(CameraServiceError::StaleProfileOperation)?;
        profile
            .report
            .clone()
            .ok_or(CameraServiceError::ProfileNotReady)
    }

    pub fn cancel_profile(&self, profile_id: &str) -> CameraServiceResult<ProfileSnapshot> {
        let wait_deadline = {
            let mut inner = self.core.lock()?;
            let profile = inner
                .last_profile
                .as_ref()
                .filter(|profile| profile.profile_id.as_str() == profile_id)
                .ok_or(CameraServiceError::StaleProfileOperation)?;
            if profile.status.is_terminal() {
                return Ok(profile.snapshot(inner.state));
            }
            let shutdown_deadline = profile.policy.shutdown_deadline();
            if let Some(control) = inner.control.as_ref() {
                control.request_user_cancel();
            }
            if let Some(profile) = inner.last_profile.as_mut() {
                profile.cancel_requested_at.get_or_insert(self.clock.now());
            }
            self.core.changed.notify_all();
            shutdown_deadline.saturating_add(Duration::from_secs(1))
        };
        self.wait_for_profile_terminal(profile_id, wait_deadline)
    }

    pub fn get(&self, operation_id: &str) -> CameraServiceResult<DeviceScanSnapshot> {
        let inner = self.core.lock()?;
        let operation = inner
            .last_scan
            .as_ref()
            .filter(|operation| operation.operation_id.as_str() == operation_id)
            .ok_or(CameraServiceError::StaleOperation)?;
        Ok(operation.snapshot(inner.state))
    }

    pub fn cancel(&self, operation_id: &str) -> CameraServiceResult<DeviceScanSnapshot> {
        let wait_deadline = {
            let mut inner = self.core.lock()?;
            let is_requested_operation = inner
                .last_scan
                .as_ref()
                .is_some_and(|operation| operation.operation_id.as_str() == operation_id);
            if !is_requested_operation {
                return Err(CameraServiceError::StaleOperation);
            }
            if inner
                .last_scan
                .as_ref()
                .is_some_and(|operation| operation.status.is_terminal())
            {
                let operation = inner
                    .last_scan
                    .as_ref()
                    .ok_or(CameraServiceError::Synchronization)?;
                return Ok(operation.snapshot(inner.state));
            }
            let shutdown_deadline = inner
                .last_scan
                .as_ref()
                .map(|operation| operation.policy.shutdown_deadline())
                .ok_or(CameraServiceError::Synchronization)?;
            if let Some(control) = inner.control.as_ref() {
                control.request_user_cancel();
            }
            if let Some(operation) = inner.last_scan.as_mut() {
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
        let (scan_id, profile_id) = {
            let mut inner = self.core.lock()?;
            match inner.state {
                CameraServiceState::Idle => {
                    drop(inner);
                    self.reap_finished_supervisor()?;
                    return self.snapshot();
                }
                CameraServiceState::ProfileReady => {
                    inner.verified_modes.invalidate();
                    inner.state = CameraServiceState::Idle;
                    drop(inner);
                    self.reap_finished_supervisor()?;
                    return self.snapshot();
                }
                CameraServiceState::Faulted => {
                    inner.verified_modes.invalidate();
                    inner.state = CameraServiceState::Idle;
                    if let Some(operation) = inner.last_scan.as_mut() {
                        operation.service_state = CameraServiceState::Idle;
                    }
                    drop(inner);
                    self.reap_finished_supervisor()?;
                    return self.snapshot();
                }
                CameraServiceState::Stuck => return Ok(inner.snapshot()),
                CameraServiceState::Scanning => (
                    inner
                        .last_scan
                        .as_ref()
                        .map(|operation| operation.operation_id.as_str().to_owned()),
                    None,
                ),
                CameraServiceState::Profiling => (
                    None,
                    inner
                        .last_profile
                        .as_ref()
                        .map(|profile| profile.profile_id.as_str().to_owned()),
                ),
            }
        };
        if let Some(operation_id) = scan_id {
            let _ = self.cancel(&operation_id)?;
        } else if let Some(profile_id) = profile_id {
            let _ = self.cancel_profile(&profile_id)?;
        } else {
            return Err(CameraServiceError::Synchronization);
        }
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
                .last_scan
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

    fn wait_for_profile_terminal(
        &self,
        profile_id: &str,
        maximum_wait: Duration,
    ) -> CameraServiceResult<ProfileSnapshot> {
        let started_at = std::time::Instant::now();
        let mut inner = self.core.lock()?;
        loop {
            let profile = inner
                .last_profile
                .as_ref()
                .filter(|profile| profile.profile_id.as_str() == profile_id)
                .ok_or(CameraServiceError::StaleProfileOperation)?;
            if profile.status.is_terminal() {
                return Ok(profile.snapshot(inner.state));
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
            if let Some(operation) = inner.last_scan.as_mut() {
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
            if let Some(operation) = inner.last_scan.as_mut() {
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
            if let Some(operation) = inner.last_scan.as_mut() {
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

    pub(crate) fn begin_profile_candidate(
        &self,
        ordinal: usize,
        tuple: ModeTuple,
        epoch: u64,
        now: Duration,
    ) {
        if let Ok(mut inner) = self.inner.lock() {
            if let Some(profile) = inner.last_profile.as_mut() {
                debug_assert_eq!(ordinal, profile.completed_candidates);
                profile.current_candidate = Some(tuple);
                profile.current_phase = Some(CandidatePhase::Opening);
                profile.phase_history.push(CandidatePhase::Opening);
                profile.current_candidate_epoch = epoch;
                profile.current_candidate_started_at = Some(now);
                profile.candidate_timeout_requested = None;
                profile.last_progress_at = now;
            }
            self.changed.notify_all();
        }
    }

    pub(crate) fn set_profile_phase(&self, phase: CandidatePhase, now: Duration) {
        if let Ok(mut inner) = self.inner.lock() {
            if let Some(profile) = inner.last_profile.as_mut() {
                profile.current_phase = Some(phase);
                profile.phase_history.push(phase);
                profile.last_progress_at = now;
            }
            self.changed.notify_all();
        }
    }

    pub(crate) fn complete_profile_candidate(&self, result: CandidateResult, now: Duration) {
        if let Ok(mut inner) = self.inner.lock() {
            if let Some(profile) = inner.last_profile.as_mut() {
                profile.results.push(result);
                profile.completed_candidates = profile.results.len();
                profile.current_candidate = None;
                profile.current_phase = None;
                profile.current_candidate_started_at = None;
                profile.candidate_timeout_requested = None;
                profile.last_progress_at = now;
            }
            self.changed.notify_all();
        }
    }
}

struct ServiceInner {
    state: CameraServiceState,
    generation: u64,
    active_operation: Option<ActiveOperationKind>,
    last_scan: Option<OperationRecord>,
    profile_generation: u64,
    last_profile: Option<ProfileRecord>,
    verified_modes: VerifiedModeRegistry,
    control: Option<Arc<OperationControl>>,
    supervisor: Option<JoinHandle<()>>,
}

impl ServiceInner {
    fn new() -> Self {
        Self {
            state: CameraServiceState::Idle,
            generation: 0,
            active_operation: None,
            last_scan: None,
            profile_generation: 0,
            last_profile: None,
            verified_modes: VerifiedModeRegistry::default(),
            control: None,
            supervisor: None,
        }
    }

    fn snapshot(&self) -> CameraServiceSnapshot {
        CameraServiceSnapshot::new(
            self.state,
            self.last_scan
                .as_ref()
                .map(|operation| operation.snapshot(self.state)),
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ActiveOperationKind {
    Scan,
    Profile,
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

struct ProfileRecord {
    profile_id: ProfileId,
    endpoint: DeviceEndpoint,
    policy: ModeCandidatePolicy,
    config_hash: String,
    status: ProfileOperationStatus,
    completed_candidates: usize,
    current_candidate: Option<ModeTuple>,
    current_phase: Option<CandidatePhase>,
    phase_history: Vec<CandidatePhase>,
    current_candidate_epoch: u64,
    results: Vec<CandidateResult>,
    failure_reason: Option<CandidateFailureReason>,
    report: Option<ProfileReport>,
    started_at: Duration,
    started_at_unix_ms: u64,
    current_candidate_started_at: Option<Duration>,
    candidate_timeout_requested: Option<(u64, Duration)>,
    cancel_requested_at: Option<Duration>,
    last_progress_at: Duration,
}

impl ProfileRecord {
    fn new(
        profile_id: ProfileId,
        endpoint: DeviceEndpoint,
        policy: ModeCandidatePolicy,
        config_hash: String,
        started_at: Duration,
        started_at_unix_ms: u64,
    ) -> Self {
        Self {
            profile_id,
            endpoint,
            policy,
            config_hash,
            status: ProfileOperationStatus::Profiling,
            completed_candidates: 0,
            current_candidate: None,
            current_phase: None,
            phase_history: Vec::new(),
            current_candidate_epoch: 0,
            results: Vec::new(),
            failure_reason: None,
            report: None,
            started_at,
            started_at_unix_ms,
            current_candidate_started_at: None,
            candidate_timeout_requested: None,
            cancel_requested_at: None,
            last_progress_at: started_at,
        }
    }

    fn snapshot(&self, service_state: CameraServiceState) -> ProfileSnapshot {
        ProfileSnapshot {
            profile_id: self.profile_id.clone(),
            service_state,
            status: self.status,
            endpoint_key: self.endpoint.key().clone(),
            scan_generation: self.endpoint.generation(),
            backend: self.endpoint.target().backend(),
            policy: self.policy.to_wire(),
            config_hash: self.config_hash.clone(),
            completed_candidates: self.completed_candidates.min(self.policy.candidate_count()),
            total_candidates: self.policy.candidate_count(),
            current_candidate: self.current_candidate,
            current_phase: self.current_phase,
            failure_reason: self.failure_reason,
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
        let Some(operation) = inner.last_scan.as_mut() else {
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

fn supervise_profile(
    core: Arc<ServiceCore>,
    adapter_factory: Arc<dyn CaptureAdapterFactory>,
    clock: Arc<dyn MonotonicClock>,
    control: Arc<OperationControl>,
    policy: ModeCandidatePolicy,
    endpoint: DeviceEndpoint,
) {
    let worker_context = ProfileWorkerContext {
        core: Arc::clone(&core),
        control: Arc::clone(&control),
        clock: Arc::clone(&clock),
        policy: policy.clone(),
        target: endpoint.target(),
    };
    let worker = thread::Builder::new()
        .name(format!("camera-profile-worker-{}", endpoint.generation()))
        .spawn(move || run_profile_worker(adapter_factory.create(), worker_context));
    let Ok(worker) = worker else {
        finalize_profile(
            &core,
            CameraServiceState::Faulted,
            ProfileOperationStatus::Failed,
            Some(CandidateFailureReason::OperationDeadline),
            false,
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
        let Some(profile) = inner.last_profile.as_mut() else {
            break;
        };
        if control.cancel_reason() == CancelReason::None
            && now.saturating_sub(profile.started_at) >= policy.operation_deadline()
        {
            control.request_operation_timeout();
            profile.cancel_requested_at.get_or_insert(now);
        }
        if let Some(candidate_started_at) = profile.current_candidate_started_at {
            let epoch = profile.current_candidate_epoch;
            if profile.candidate_timeout_requested.is_none()
                && now.saturating_sub(candidate_started_at) >= policy.candidate_deadline()
            {
                control.request_probe_timeout(epoch);
                profile.candidate_timeout_requested = Some((epoch, now));
            }
        }
        let cancellation_stalled = profile.cancel_requested_at.is_some_and(|requested_at| {
            now.saturating_sub(requested_at) >= policy.shutdown_deadline()
        });
        let candidate_stalled =
            profile
                .candidate_timeout_requested
                .is_some_and(|(epoch, requested_at)| {
                    epoch == profile.current_candidate_epoch
                        && profile.current_candidate.is_some()
                        && now.saturating_sub(requested_at) >= policy.shutdown_deadline()
                });
        if cancellation_stalled || candidate_stalled {
            profile.status = ProfileOperationStatus::Stuck;
            profile.failure_reason = Some(CandidateFailureReason::ReadStalled);
            profile.current_phase = None;
            inner.state = CameraServiceState::Stuck;
            inner.active_operation = None;
            inner.control = None;
            build_profile_report_locked(&mut inner, ProfileTerminalStatus::Stuck);
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
        Ok(ProfileWorkerResult::Completed(results)) => {
            let _ = results;
            finalize_profile(
                &core,
                CameraServiceState::ProfileReady,
                ProfileOperationStatus::Completed,
                None,
                true,
            );
        }
        Ok(ProfileWorkerResult::Cancelled(results)) => {
            let _ = results;
            finalize_profile(
                &core,
                CameraServiceState::Idle,
                ProfileOperationStatus::Cancelled,
                Some(CandidateFailureReason::Cancelled),
                false,
            );
        }
        Ok(ProfileWorkerResult::OperationTimedOut(results)) => {
            let _ = results;
            finalize_profile(
                &core,
                CameraServiceState::Faulted,
                ProfileOperationStatus::Failed,
                Some(CandidateFailureReason::OperationDeadline),
                false,
            );
        }
        Ok(ProfileWorkerResult::ReleaseFailed { results, error }) => {
            let _ = (results, error);
            finalize_profile(
                &core,
                CameraServiceState::Stuck,
                ProfileOperationStatus::Stuck,
                Some(CandidateFailureReason::ReleaseFailed),
                false,
            );
        }
        Err(_) => finalize_profile(
            &core,
            CameraServiceState::Stuck,
            ProfileOperationStatus::Stuck,
            Some(CandidateFailureReason::ReadStalled),
            false,
        ),
    }
}

fn finalize_profile(
    core: &ServiceCore,
    service_state: CameraServiceState,
    status: ProfileOperationStatus,
    failure_reason: Option<CandidateFailureReason>,
    issue_verified_modes: bool,
) {
    if let Ok(mut inner) = core.inner.lock() {
        if inner.state == CameraServiceState::Stuck && service_state != CameraServiceState::Stuck {
            return;
        }
        inner.state = service_state;
        inner.active_operation = None;
        inner.control = None;
        if let Some(profile) = inner.last_profile.as_mut() {
            profile.status = status;
            profile.failure_reason = failure_reason;
            profile.current_candidate = None;
            profile.current_phase = None;
            profile.current_candidate_started_at = None;
        }
        if issue_verified_modes {
            issue_verified_modes_locked(&mut inner);
        }
        let terminal_status = match status {
            ProfileOperationStatus::Completed => ProfileTerminalStatus::Completed,
            ProfileOperationStatus::Cancelled => ProfileTerminalStatus::Cancelled,
            ProfileOperationStatus::Failed => ProfileTerminalStatus::Failed,
            ProfileOperationStatus::Stuck => ProfileTerminalStatus::Stuck,
            ProfileOperationStatus::Profiling => return,
        };
        build_profile_report_locked(&mut inner, terminal_status);
        core.changed.notify_all();
    }
}

fn issue_verified_modes_locked(inner: &mut ServiceInner) {
    let Some(profile) = inner.last_profile.as_mut() else {
        return;
    };
    for result in &mut profile.results {
        let descriptor = VerifiedModeDescriptor {
            profile_id: profile.profile_id.clone(),
            endpoint_key: profile.endpoint.key().clone(),
            scan_generation: profile.endpoint.generation(),
            backend: profile.endpoint.target().backend(),
            tuple: result.tuple,
            config_hash: profile.config_hash.clone(),
        };
        let _ = inner.verified_modes.issue(result, descriptor);
    }
}

fn build_profile_report_locked(inner: &mut ServiceInner, status: ProfileTerminalStatus) {
    let Some(profile) = inner.last_profile.as_mut() else {
        return;
    };
    let report = ProfileReport::finalize(ProfileReportInput {
        profile_id: profile.profile_id.clone(),
        started_at_unix_ms: profile.started_at_unix_ms,
        environment: environment_versions(),
        endpoint_key: profile.endpoint.key().clone(),
        scan_generation: profile.endpoint.generation(),
        backend: profile.endpoint.target().backend(),
        policy: profile.policy.clone(),
        status,
        failure_reason: profile.failure_reason,
        results: profile.results.clone(),
    });
    profile.report = report.ok();
}

fn environment_versions() -> EnvironmentVersionReferences {
    EnvironmentVersionReferences {
        app_version: env!("CARGO_PKG_VERSION").to_owned(),
        rust_version: "1.98.1".to_owned(),
        tauri_version: "2.11.5".to_owned(),
        opencv_version: "4.12.0".to_owned(),
        opencv_crate_version: "0.100.1".to_owned(),
    }
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
}

fn set_terminal(
    core: &ServiceCore,
    service_state: CameraServiceState,
    status: DeviceScanStatus,
    failure_code: Option<CameraFailureCode>,
) {
    if let Ok(mut inner) = core.inner.lock() {
        inner.state = service_state;
        inner.active_operation = None;
        inner.control = None;
        if let Some(operation) = inner.last_scan.as_mut() {
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
        profiling::{
            FrameMetadata, ModeCandidatePolicyV1, ModeTuple, ProfileOperationStatus,
            PropertySetDiagnostics, ReportedCaptureProperties, ResolutionV1,
        },
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
        session_calls: Mutex<Vec<&'static str>>,
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

    struct AdvancingClock {
        milliseconds: AtomicU64,
    }

    impl AdvancingClock {
        fn new() -> Self {
            Self {
                milliseconds: AtomicU64::new(0),
            }
        }
    }

    impl MonotonicClock for AdvancingClock {
        fn now(&self) -> Duration {
            Duration::from_millis(self.milliseconds.fetch_add(1, Ordering::SeqCst))
        }

        fn sleep(&self, duration: Duration) {
            self.milliseconds.fetch_add(
                u64::try_from(duration.as_millis()).unwrap_or(1).max(1),
                Ordering::SeqCst,
            );
            thread::yield_now();
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
                session_calls: Mutex::new(Vec::new()),
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
        fn apply_mode(&mut self, _mode: ModeTuple) -> CaptureAdapterResult<PropertySetDiagnostics> {
            self.state
                .session_calls
                .lock()
                .map_err(|_| marker_open())?
                .push("apply");
            Ok(successful_set_diagnostics())
        }

        fn reported_properties(&mut self) -> CaptureAdapterResult<ReportedCaptureProperties> {
            self.state
                .session_calls
                .lock()
                .map_err(|_| marker_open())?
                .push("get");
            Ok(unavailable_reported_properties())
        }

        fn read(&mut self) -> CaptureAdapterResult<FrameRead> {
            self.state
                .session_calls
                .lock()
                .map_err(|_| marker_read())?
                .push("read");
            match self.reads.pop_front().unwrap_or(Ok(FrameRead::Empty)) {
                Ok(value) => Ok(value),
                Err(()) => Err(CaptureAdapterError::read(io::Error::other("READ_MARKER"))),
            }
        }

        fn release(&mut self) -> CaptureAdapterResult<()> {
            self.state
                .session_calls
                .lock()
                .map_err(|_| marker_release())?
                .push("release");
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
        fn apply_mode(&mut self, _mode: ModeTuple) -> CaptureAdapterResult<PropertySetDiagnostics> {
            Ok(successful_set_diagnostics())
        }

        fn reported_properties(&mut self) -> CaptureAdapterResult<ReportedCaptureProperties> {
            Ok(unavailable_reported_properties())
        }

        fn read(&mut self) -> CaptureAdapterResult<FrameRead> {
            let (lock, changed) = &*self.gate;
            let mut ready = lock.lock().map_err(|_| marker_read())?;
            while !*ready {
                ready = changed.wait(ready).map_err(|_| marker_read())?;
            }
            Ok(frame())
        }

        fn release(&mut self) -> CaptureAdapterResult<()> {
            release_script_session(&self.state, &mut self.released)
        }
    }

    fn successful_set_diagnostics() -> PropertySetDiagnostics {
        PropertySetDiagnostics {
            fourcc: true,
            width: true,
            height: true,
            fps: true,
        }
    }

    fn unavailable_reported_properties() -> ReportedCaptureProperties {
        ReportedCaptureProperties {
            fourcc: None,
            width: None,
            height: None,
            fps: None,
        }
    }

    fn frame() -> FrameRead {
        FrameRead::Frame(FrameMetadata {
            width: 640,
            height: 480,
        })
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

    fn profile_policy() -> ModeCandidatePolicyV1 {
        ModeCandidatePolicyV1 {
            schema_version: 1,
            fourcc: vec!["MJPG".into()],
            resolutions: vec![ResolutionV1 {
                width: 640,
                height: 480,
            }],
            fps: vec![30.0],
            warmup_ms: 1,
            capture_only_ms: 5,
            first_frame_deadline_ms: 10,
            candidate_deadline_ms: 1_000,
            operation_deadline_ms: 10_000,
            shutdown_deadline_ms: 100,
            reopen_delay_ms: 1,
            minimum_fps_ratio: 0.1,
            maximum_read_failure_ratio: 1.0,
            maximum_gap_periods: 100.0,
            maximum_long_gap_ratio: 1.0,
        }
    }

    fn profile_policy_with_fps(fps: Vec<f64>) -> ModeCandidatePolicyV1 {
        ModeCandidatePolicyV1 {
            fps,
            ..profile_policy()
        }
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

    fn wait_for_profile_terminal(service: &CameraService, profile_id: &str) -> ProfileSnapshot {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let snapshot = service
                .get_profile_status(profile_id)
                .expect("profile snapshot is available");
            if snapshot.status.is_terminal() {
                return snapshot;
            }
            assert!(Instant::now() < deadline, "profile worker did not finish");
            thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn invalid_profile_config_is_rejected_before_camera_adapter_calls() {
        let state = ScriptState::new([OpenScript::OpenError]);
        let service = CameraService::new(Arc::new(ScriptFactory(Arc::clone(&state))));
        let mut invalid = profile_policy();
        invalid.schema_version = 99;

        assert!(matches!(
            service.start_profile("stale", invalid),
            Err(CameraServiceError::InvalidProfileConfig(
                crate::camera::profiling::ProfileConfigError::UnsupportedSchema
            ))
        ));
        assert!(state.targets.lock().expect("targets lock").is_empty());
        assert!(state.session_calls.lock().expect("calls lock").is_empty());
    }

    #[test]
    fn profile_uses_current_endpoint_ordered_session_calls_and_profile_ready_state() {
        let profile_reads = (0..20).map(|_| Ok(frame())).collect::<Vec<_>>();
        let state = ScriptState::new([
            OpenScript::Session {
                reads: vec![Ok(frame())],
                release_error: false,
            },
            OpenScript::Session {
                reads: profile_reads,
                release_error: false,
            },
        ]);
        let service = CameraService::with_clock(
            Arc::new(ScriptFactory(Arc::clone(&state))),
            Arc::new(AdvancingClock::new()),
        );
        let scan = service
            .start(policy(vec![CaptureBackend::Dshow], 0))
            .expect("scan starts");
        let scan = wait_for_terminal(&service, scan.operation_id().as_str());
        let endpoint = scan.endpoints().first().expect("endpoint");

        let started = service
            .start_profile(endpoint.key().as_str(), profile_policy())
            .expect("profile starts");
        let terminal = wait_for_profile_terminal(&service, started.profile_id.as_str());
        assert_eq!(terminal.status, ProfileOperationStatus::Completed);
        assert_eq!(terminal.service_state, CameraServiceState::ProfileReady);
        assert_eq!(terminal.completed_candidates, 1);
        let report = service
            .get_profile_result(started.profile_id.as_str())
            .expect("terminal report");
        assert_eq!(report.results.len(), 1);
        assert_eq!(*state.maximum_active.lock().expect("maximum lock"), 1);

        let calls = state.session_calls.lock().expect("calls lock");
        let apply = calls
            .iter()
            .position(|call| *call == "apply")
            .expect("apply");
        let get = calls.iter().position(|call| *call == "get").expect("get");
        let release = calls
            .iter()
            .rposition(|call| *call == "release")
            .expect("release");
        assert!(apply < get && get < release);
        assert!(calls[apply + 2..release].contains(&"read"));
    }

    #[test]
    fn profile_rejects_stale_endpoint_without_opening_another_session() {
        let state = ScriptState::new([OpenScript::Session {
            reads: vec![Ok(frame())],
            release_error: false,
        }]);
        let service = CameraService::new(Arc::new(ScriptFactory(Arc::clone(&state))));
        let scan = service
            .start(policy(vec![CaptureBackend::Dshow], 0))
            .expect("scan starts");
        let _ = wait_for_terminal(&service, scan.operation_id().as_str());
        let opens_before = state.targets.lock().expect("targets lock").len();
        assert!(matches!(
            service.start_profile("endpoint-from-another-process", profile_policy()),
            Err(CameraServiceError::StaleDeviceEndpoint)
        ));
        assert_eq!(
            state.targets.lock().expect("targets lock").len(),
            opens_before
        );
    }

    #[test]
    fn scan_and_profile_are_mutually_exclusive_and_profile_cancel_waits_for_release() {
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let state = ScriptState::new([
            OpenScript::Session {
                reads: vec![Ok(frame())],
                release_error: false,
            },
            OpenScript::Blocking {
                gate: Arc::clone(&gate),
            },
        ]);
        let service = CameraService::new(Arc::new(ScriptFactory(Arc::clone(&state))));
        let scan = service
            .start(policy(vec![CaptureBackend::Dshow], 0))
            .expect("scan starts");
        let scan = wait_for_terminal(&service, scan.operation_id().as_str());
        let endpoint = scan.endpoints()[0].key().as_str().to_owned();
        let mut long_policy = profile_policy();
        long_policy.first_frame_deadline_ms = 5_000;
        long_policy.candidate_deadline_ms = 6_000;
        long_policy.operation_deadline_ms = 10_000;
        long_policy.shutdown_deadline_ms = 1_000;
        let profile = service
            .start_profile(&endpoint, long_policy.clone())
            .expect("profile starts");
        assert!(matches!(
            service.start(policy(vec![CaptureBackend::Dshow], 0)),
            Err(CameraServiceError::Busy)
        ));
        assert!(matches!(
            service.start_profile(&endpoint, long_policy),
            Err(CameraServiceError::Busy)
        ));

        let cancelling_service = service.clone();
        let profile_id = profile.profile_id.as_str().to_owned();
        let cancelling = thread::spawn(move || {
            cancelling_service
                .cancel_profile(&profile_id)
                .expect("cancel completes")
        });
        thread::sleep(Duration::from_millis(10));
        let (lock, changed) = &*gate;
        *lock.lock().expect("gate lock") = true;
        changed.notify_all();
        let cancelled = cancelling.join().expect("cancel thread");
        assert_eq!(cancelled.status, ProfileOperationStatus::Cancelled);
        assert_eq!(cancelled.service_state, CameraServiceState::Idle);
        assert_eq!(*state.active.lock().expect("active lock"), 0);
    }

    #[test]
    fn profile_retries_open_once_and_never_opens_sessions_concurrently() {
        let state = ScriptState::new([
            OpenScript::Session {
                reads: vec![Ok(frame())],
                release_error: false,
            },
            OpenScript::OpenError,
            OpenScript::Session {
                reads: (0..20).map(|_| Ok(frame())).collect(),
                release_error: false,
            },
        ]);
        let service = CameraService::with_clock(
            Arc::new(ScriptFactory(Arc::clone(&state))),
            Arc::new(AdvancingClock::new()),
        );
        let scan = service
            .start(policy(vec![CaptureBackend::Dshow], 0))
            .expect("scan starts");
        let scan = wait_for_terminal(&service, scan.operation_id().as_str());
        let profile = service
            .start_profile(scan.endpoints()[0].key().as_str(), profile_policy())
            .expect("profile starts");
        let _ = wait_for_profile_terminal(&service, profile.profile_id.as_str());
        let report = service
            .get_profile_result(profile.profile_id.as_str())
            .expect("report");
        assert_eq!(report.results[0].attempt_count, 2);
        assert_eq!(
            report.results[0].retry_reason,
            Some(crate::camera::profiling::CandidateRetryReason::OpenFailed)
        );
        assert_eq!(state.targets.lock().expect("targets lock").len(), 3);
        assert_eq!(*state.maximum_active.lock().expect("maximum lock"), 1);
    }

    #[test]
    fn profile_retries_first_read_start_once_and_second_attempt_can_pass() {
        let state = ScriptState::new([
            OpenScript::Session {
                reads: vec![Ok(frame())],
                release_error: false,
            },
            OpenScript::Session {
                reads: vec![Err(())],
                release_error: false,
            },
            OpenScript::Session {
                reads: (0..20).map(|_| Ok(frame())).collect(),
                release_error: false,
            },
        ]);
        let service = CameraService::with_clock(
            Arc::new(ScriptFactory(Arc::clone(&state))),
            Arc::new(AdvancingClock::new()),
        );
        let scan = service
            .start(policy(vec![CaptureBackend::Dshow], 0))
            .expect("scan starts");
        let scan = wait_for_terminal(&service, scan.operation_id().as_str());
        let profile = service
            .start_profile(scan.endpoints()[0].key().as_str(), profile_policy())
            .expect("profile starts");
        let _ = wait_for_profile_terminal(&service, profile.profile_id.as_str());
        let report = service
            .get_profile_result(profile.profile_id.as_str())
            .expect("report");

        assert_eq!(report.results[0].attempt_count, 2);
        assert_eq!(
            report.results[0].retry_reason,
            Some(crate::camera::profiling::CandidateRetryReason::FirstReadStartFailed)
        );
        assert_ne!(
            report.results[0].status,
            crate::camera::profiling::CaptureModeStatus::FirstFrameTimeout
        );
        assert_eq!(state.targets.lock().expect("targets lock").len(), 3);
        assert_eq!(*state.maximum_active.lock().expect("maximum lock"), 1);
    }

    #[test]
    fn profile_does_not_retry_measurement_failure() {
        let state = ScriptState::new([
            OpenScript::Session {
                reads: vec![Ok(frame())],
                release_error: false,
            },
            OpenScript::Session {
                reads: vec![Ok(frame()), Err(())],
                release_error: false,
            },
        ]);
        let service = CameraService::with_clock(
            Arc::new(ScriptFactory(Arc::clone(&state))),
            Arc::new(AdvancingClock::new()),
        );
        let scan = service
            .start(policy(vec![CaptureBackend::Dshow], 0))
            .expect("scan starts");
        let scan = wait_for_terminal(&service, scan.operation_id().as_str());
        let profile = service
            .start_profile(scan.endpoints()[0].key().as_str(), profile_policy())
            .expect("profile starts");
        let _ = wait_for_profile_terminal(&service, profile.profile_id.as_str());
        let report = service
            .get_profile_result(profile.profile_id.as_str())
            .expect("report");

        assert_eq!(report.results[0].attempt_count, 1);
        assert_eq!(report.results[0].retry_reason, None);
        assert!(!matches!(
            report.results[0].status,
            crate::camera::profiling::CaptureModeStatus::OpeningFailed
                | crate::camera::profiling::CaptureModeStatus::FirstFrameTimeout
        ));
        assert_eq!(state.targets.lock().expect("targets lock").len(), 2);
    }

    #[test]
    fn profile_retry_never_opens_again_after_candidate_deadline() {
        let state = ScriptState::new([
            OpenScript::Session {
                reads: vec![Ok(frame())],
                release_error: false,
            },
            OpenScript::OpenError,
        ]);
        let clock = Arc::new(ManualClock::new());
        let service = CameraService::with_clock(
            Arc::new(ScriptFactory(Arc::clone(&state))),
            Arc::clone(&clock) as Arc<dyn MonotonicClock>,
        );
        let scan = service
            .start(policy(vec![CaptureBackend::Dshow], 0))
            .expect("scan starts");
        let scan = wait_for_terminal(&service, scan.operation_id().as_str());
        let mut deadline_policy = profile_policy();
        deadline_policy.warmup_ms = 1;
        deadline_policy.capture_only_ms = 1;
        deadline_policy.first_frame_deadline_ms = 1;
        deadline_policy.candidate_deadline_ms = 4;
        deadline_policy.operation_deadline_ms = 10_000;
        deadline_policy.shutdown_deadline_ms = 1;
        deadline_policy.reopen_delay_ms = 5;
        let profile = service
            .start_profile(scan.endpoints()[0].key().as_str(), deadline_policy)
            .expect("profile starts");
        let retry_deadline = Instant::now() + Duration::from_secs(1);
        while state.targets.lock().expect("targets lock").len() < 2 {
            assert!(
                Instant::now() < retry_deadline,
                "first attempt did not open"
            );
            thread::sleep(Duration::from_millis(1));
        }
        clock.advance(5);
        let _ = wait_for_profile_terminal(&service, profile.profile_id.as_str());
        let report = service
            .get_profile_result(profile.profile_id.as_str())
            .expect("report");

        assert_eq!(report.results[0].attempt_count, 2);
        assert_eq!(
            report.results[0].status,
            crate::camera::profiling::CaptureModeStatus::CandidateTimedOut
        );
        assert_eq!(state.targets.lock().expect("targets lock").len(), 2);
    }

    #[test]
    fn candidate_timeout_releases_and_continues_with_remaining_tuples() {
        let state = ScriptState::new([
            OpenScript::Session {
                reads: vec![Ok(frame())],
                release_error: false,
            },
            OpenScript::Session {
                reads: vec![Ok(frame())],
                release_error: false,
            },
            OpenScript::Session {
                reads: vec![Ok(frame())],
                release_error: false,
            },
        ]);
        let service = CameraService::with_clock(
            Arc::new(ScriptFactory(Arc::clone(&state))),
            Arc::new(AdvancingClock::new()),
        );
        let scan = service
            .start(policy(vec![CaptureBackend::Dshow], 0))
            .expect("scan starts");
        let scan = wait_for_terminal(&service, scan.operation_id().as_str());
        let mut timeout_policy = profile_policy_with_fps(vec![30.0, 15.0]);
        timeout_policy.warmup_ms = 1;
        timeout_policy.capture_only_ms = 4;
        timeout_policy.first_frame_deadline_ms = 1;
        timeout_policy.candidate_deadline_ms = 10;
        timeout_policy.operation_deadline_ms = 10_000;
        timeout_policy.shutdown_deadline_ms = 1;
        timeout_policy.reopen_delay_ms = 1;
        let profile = service
            .start_profile(scan.endpoints()[0].key().as_str(), timeout_policy)
            .expect("profile starts");
        let terminal = wait_for_profile_terminal(&service, profile.profile_id.as_str());
        let report = service
            .get_profile_result(profile.profile_id.as_str())
            .expect("report");

        assert_eq!(terminal.completed_candidates, 2);
        assert_eq!(report.results.len(), 2);
        assert!(report.results.iter().all(|result| {
            result.status == crate::camera::profiling::CaptureModeStatus::CandidateTimedOut
        }));
        assert_eq!(*state.releases.lock().expect("release count"), 3);
        assert_eq!(*state.maximum_active.lock().expect("maximum lock"), 1);
    }

    #[test]
    fn stop_is_idempotent_and_only_latest_profile_id_remains_addressable() {
        let profile_reads = || (0..20).map(|_| Ok(frame())).collect::<Vec<_>>();
        let state = ScriptState::new([
            OpenScript::Session {
                reads: vec![Ok(frame())],
                release_error: false,
            },
            OpenScript::Session {
                reads: profile_reads(),
                release_error: false,
            },
            OpenScript::Session {
                reads: vec![Ok(frame())],
                release_error: false,
            },
            OpenScript::Session {
                reads: profile_reads(),
                release_error: false,
            },
        ]);
        let service = CameraService::with_clock(
            Arc::new(ScriptFactory(state)),
            Arc::new(AdvancingClock::new()),
        );
        let first_scan = service
            .start(policy(vec![CaptureBackend::Dshow], 0))
            .expect("first scan starts");
        let first_scan = wait_for_terminal(&service, first_scan.operation_id().as_str());
        let first_profile = service
            .start_profile(first_scan.endpoints()[0].key().as_str(), profile_policy())
            .expect("first profile starts");
        let _ = wait_for_profile_terminal(&service, first_profile.profile_id.as_str());
        assert!(
            service
                .get_profile_result(first_profile.profile_id.as_str())
                .is_ok()
        );

        assert_eq!(
            service.stop().expect("first stop").service_state(),
            CameraServiceState::Idle
        );
        assert_eq!(
            service.stop().expect("second stop").service_state(),
            CameraServiceState::Idle
        );
        assert!(
            service
                .get_profile_result(first_profile.profile_id.as_str())
                .is_ok(),
            "stop invalidates verified IDs but preserves the historical report"
        );

        let second_scan = service
            .start(policy(vec![CaptureBackend::Dshow], 0))
            .expect("second scan starts");
        let second_scan = wait_for_terminal(&service, second_scan.operation_id().as_str());
        let second_profile = service
            .start_profile(second_scan.endpoints()[0].key().as_str(), profile_policy())
            .expect("second profile starts");
        let _ = wait_for_profile_terminal(&service, second_profile.profile_id.as_str());
        assert!(matches!(
            service.get_profile_result(first_profile.profile_id.as_str()),
            Err(CameraServiceError::StaleProfileOperation)
        ));
        assert!(
            service
                .get_profile_result(second_profile.profile_id.as_str())
                .is_ok()
        );
    }

    #[test]
    fn profile_opens_each_tuple_separately_and_publishes_ordered_phases() {
        let state = ScriptState::new([
            OpenScript::Session {
                reads: vec![Ok(frame())],
                release_error: false,
            },
            OpenScript::Session {
                reads: (0..20).map(|_| Ok(frame())).collect(),
                release_error: false,
            },
            OpenScript::Session {
                reads: (0..20).map(|_| Ok(frame())).collect(),
                release_error: false,
            },
        ]);
        let service = CameraService::with_clock(
            Arc::new(ScriptFactory(Arc::clone(&state))),
            Arc::new(AdvancingClock::new()),
        );
        let scan = service
            .start(policy(vec![CaptureBackend::Dshow], 0))
            .expect("scan starts");
        let scan = wait_for_terminal(&service, scan.operation_id().as_str());
        let profile = service
            .start_profile(
                scan.endpoints()[0].key().as_str(),
                profile_policy_with_fps(vec![30.0, 15.0]),
            )
            .expect("profile starts");
        let terminal = wait_for_profile_terminal(&service, profile.profile_id.as_str());
        assert_eq!(terminal.completed_candidates, 2);
        assert_eq!(state.targets.lock().expect("targets lock").len(), 3);
        assert_eq!(*state.releases.lock().expect("release lock"), 3);
        let inner = service.core.inner.lock().expect("service lock");
        let phases = &inner.last_profile.as_ref().expect("profile").phase_history;
        let expected_prefix = [
            CandidatePhase::Opening,
            CandidatePhase::ApplyingProperties,
            CandidatePhase::ReadingReportedProperties,
            CandidatePhase::FirstFrame,
            CandidatePhase::Warmup,
            CandidatePhase::Measuring,
            CandidatePhase::Release,
            CandidatePhase::ReopenDelay,
            CandidatePhase::Opening,
        ];
        assert_eq!(&phases[..expected_prefix.len()], &expected_prefix);
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
                reads: vec![Ok(frame())],
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
                reads: vec![Ok(frame())],
                release_error: false,
            },
            OpenScript::Session {
                reads: vec![Ok(frame())],
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
                reads: vec![Ok(frame())],
                release_error: false,
            },
            OpenScript::Session {
                reads: vec![Ok(frame())],
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
                .last_scan
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
    fn blocked_profile_call_becomes_sticky_stuck_and_keeps_join_ownership() {
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let state = ScriptState::new([
            OpenScript::Session {
                reads: vec![Ok(frame())],
                release_error: false,
            },
            OpenScript::Blocking {
                gate: Arc::clone(&gate),
            },
        ]);
        let clock = Arc::new(ManualClock::new());
        let service = CameraService::with_clock(
            Arc::new(ScriptFactory(Arc::clone(&state))),
            Arc::clone(&clock) as Arc<dyn MonotonicClock>,
        );
        let scan = service
            .start(policy(vec![CaptureBackend::Dshow], 0))
            .expect("scan starts");
        let scan = wait_for_terminal(&service, scan.operation_id().as_str());
        let mut stuck_policy = profile_policy();
        stuck_policy.first_frame_deadline_ms = 5;
        stuck_policy.candidate_deadline_ms = 12;
        stuck_policy.operation_deadline_ms = 500;
        stuck_policy.shutdown_deadline_ms = 5;
        let profile = service
            .start_profile(scan.endpoints()[0].key().as_str(), stuck_policy)
            .expect("profile starts");
        let candidate_deadline = Instant::now() + Duration::from_secs(1);
        while service
            .get_profile_status(profile.profile_id.as_str())
            .expect("profile snapshot")
            .current_candidate
            .is_none()
        {
            assert!(
                Instant::now() < candidate_deadline,
                "candidate did not begin"
            );
            thread::sleep(Duration::from_millis(1));
        }

        clock.advance(13);
        let timeout_deadline = Instant::now() + Duration::from_secs(1);
        loop {
            let requested = service
                .core
                .inner
                .lock()
                .expect("service lock")
                .last_profile
                .as_ref()
                .is_some_and(|operation| operation.candidate_timeout_requested.is_some());
            if requested {
                break;
            }
            assert!(
                Instant::now() < timeout_deadline,
                "profile watchdog did not observe fake candidate deadline"
            );
            thread::sleep(Duration::from_millis(1));
        }
        clock.advance(6);
        let stuck = wait_for_profile_terminal(&service, profile.profile_id.as_str());
        assert_eq!(stuck.status, ProfileOperationStatus::Stuck);
        assert_eq!(stuck.service_state, CameraServiceState::Stuck);
        assert!(matches!(
            service.start_profile(scan.endpoints()[0].key().as_str(), profile_policy()),
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
            "stuck profile join ownership must remain with the service"
        );

        let (lock, changed) = &*gate;
        *lock.lock().expect("gate lock") = true;
        changed.notify_all();
        thread::sleep(Duration::from_millis(20));
        assert_eq!(
            service.snapshot().expect("snapshot").service_state(),
            CameraServiceState::Stuck
        );
    }

    #[test]
    fn release_failure_never_claims_successful_cleanup() {
        let state = ScriptState::new([OpenScript::Session {
            reads: vec![Ok(frame())],
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
