use std::{collections::HashSet, fmt, time::Duration};

use crate::camera::error::CameraConfigError;

pub const DEFAULT_FIRST_INDEX: u32 = 0;
pub const DEFAULT_LAST_INDEX: u32 = 5;
pub const DEFAULT_FIRST_FRAME_DEADLINE_MS: u64 = 5_000;
pub const DEFAULT_OPERATION_DEADLINE_MS: u64 = 90_000;
pub const DEFAULT_SHUTDOWN_DEADLINE_MS: u64 = 3_000;
pub const DEFAULT_REOPEN_DELAY_MS: u64 = 500;
pub const MAX_INDICES: u32 = 32;
pub const MAX_PROBES: usize = 64;
pub const MAX_DURATION_MS: u64 = 10 * 60 * 1_000;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CaptureBackend {
    Msmf,
    Dshow,
}

impl CaptureBackend {
    pub fn name(self) -> &'static str {
        match self {
            Self::Msmf => "MSMF",
            Self::Dshow => "DSHOW",
        }
    }
}

impl fmt::Display for CaptureBackend {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeviceScanPolicy {
    first_index: u32,
    last_index: u32,
    backends: Vec<CaptureBackend>,
    first_frame_deadline: Duration,
    operation_deadline: Duration,
    shutdown_deadline: Duration,
    reopen_delay: Duration,
}

impl DeviceScanPolicy {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        first_index: u32,
        last_index: u32,
        backends: Vec<CaptureBackend>,
        first_frame_deadline_ms: u64,
        operation_deadline_ms: u64,
        shutdown_deadline_ms: u64,
        reopen_delay_ms: u64,
    ) -> Result<Self, CameraConfigError> {
        if first_index > last_index {
            return Err(CameraConfigError::ReversedRange {
                first_index,
                last_index,
            });
        }
        let index_count = last_index
            .checked_sub(first_index)
            .and_then(|difference| difference.checked_add(1))
            .ok_or(CameraConfigError::RangeTooLarge { limit: MAX_INDICES })?;
        if index_count > MAX_INDICES {
            return Err(CameraConfigError::RangeTooLarge { limit: MAX_INDICES });
        }
        if backends.is_empty() {
            return Err(CameraConfigError::EmptyBackends);
        }
        let mut unique = HashSet::with_capacity(backends.len());
        if backends
            .iter()
            .copied()
            .any(|backend| !unique.insert(backend))
        {
            return Err(CameraConfigError::DuplicateBackend);
        }
        let total_probes = usize::try_from(index_count)
            .ok()
            .and_then(|count| count.checked_mul(backends.len()))
            .ok_or(CameraConfigError::TooManyProbes { limit: MAX_PROBES })?;
        if total_probes > MAX_PROBES {
            return Err(CameraConfigError::TooManyProbes { limit: MAX_PROBES });
        }

        validate_duration("first_frame_deadline_ms", first_frame_deadline_ms)?;
        validate_duration("operation_deadline_ms", operation_deadline_ms)?;
        validate_duration("shutdown_deadline_ms", shutdown_deadline_ms)?;
        validate_duration("reopen_delay_ms", reopen_delay_ms)?;
        if operation_deadline_ms <= first_frame_deadline_ms {
            return Err(CameraConfigError::InconsistentDurations {
                message: "operation_deadline_ms must be greater than first_frame_deadline_ms",
            });
        }
        if shutdown_deadline_ms > operation_deadline_ms {
            return Err(CameraConfigError::InconsistentDurations {
                message: "shutdown_deadline_ms must not exceed operation_deadline_ms",
            });
        }

        Ok(Self {
            first_index,
            last_index,
            backends,
            first_frame_deadline: Duration::from_millis(first_frame_deadline_ms),
            operation_deadline: Duration::from_millis(operation_deadline_ms),
            shutdown_deadline: Duration::from_millis(shutdown_deadline_ms),
            reopen_delay: Duration::from_millis(reopen_delay_ms),
        })
    }

    pub fn defaults() -> Self {
        Self {
            first_index: DEFAULT_FIRST_INDEX,
            last_index: DEFAULT_LAST_INDEX,
            backends: vec![CaptureBackend::Msmf, CaptureBackend::Dshow],
            first_frame_deadline: Duration::from_millis(DEFAULT_FIRST_FRAME_DEADLINE_MS),
            operation_deadline: Duration::from_millis(DEFAULT_OPERATION_DEADLINE_MS),
            shutdown_deadline: Duration::from_millis(DEFAULT_SHUTDOWN_DEADLINE_MS),
            reopen_delay: Duration::from_millis(DEFAULT_REOPEN_DELAY_MS),
        }
    }

    pub fn first_index(&self) -> u32 {
        self.first_index
    }

    pub fn last_index(&self) -> u32 {
        self.last_index
    }

    pub fn backends(&self) -> &[CaptureBackend] {
        &self.backends
    }

    pub fn first_frame_deadline(&self) -> Duration {
        self.first_frame_deadline
    }

    pub fn operation_deadline(&self) -> Duration {
        self.operation_deadline
    }

    pub fn shutdown_deadline(&self) -> Duration {
        self.shutdown_deadline
    }

    pub fn reopen_delay(&self) -> Duration {
        self.reopen_delay
    }

    pub fn targets(&self) -> Vec<ProbeTarget> {
        self.backends
            .iter()
            .copied()
            .flat_map(|backend| {
                (self.first_index..=self.last_index)
                    .map(move |numeric_index| ProbeTarget::new(backend, numeric_index))
            })
            .collect()
    }

    pub fn total_probes(&self) -> usize {
        self.targets().len()
    }
}

fn validate_duration(field: &'static str, value: u64) -> Result<(), CameraConfigError> {
    if value == 0 || value > MAX_DURATION_MS {
        return Err(CameraConfigError::InvalidDuration {
            field,
            maximum_ms: MAX_DURATION_MS,
        });
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProbeTarget {
    backend: CaptureBackend,
    numeric_index: u32,
}

impl ProbeTarget {
    pub fn new(backend: CaptureBackend, numeric_index: u32) -> Self {
        Self {
            backend,
            numeric_index,
        }
    }

    pub fn backend(self) -> CaptureBackend {
        self.backend
    }

    pub fn numeric_index(self) -> u32 {
        self.numeric_index
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CameraServiceState {
    Idle,
    Scanning,
    Faulted,
    Stuck,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeviceScanStatus {
    Scanning,
    Completed,
    Cancelled,
    Failed,
    Stuck,
}

impl DeviceScanStatus {
    pub fn is_terminal(self) -> bool {
        !matches!(self, Self::Scanning)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProbeStatus {
    Available,
    OpenFailed,
    FirstFrameTimeout,
    ReadFailed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProbeOutcome {
    target: ProbeTarget,
    status: ProbeStatus,
}

impl ProbeOutcome {
    pub fn new(target: ProbeTarget, status: ProbeStatus) -> Self {
        Self { target, status }
    }

    pub fn target(self) -> ProbeTarget {
        self.target
    }

    pub fn status(self) -> ProbeStatus {
        self.status
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationId(String);

impl OperationId {
    pub(crate) fn new(service_instance: u64, generation: u64) -> Self {
        Self(format!("scan-{service_instance:016x}-{generation:016x}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeviceEndpointKey(String);

impl DeviceEndpointKey {
    pub(crate) fn new(service_instance: u64, generation: u64, target: ProbeTarget) -> Self {
        Self(format!(
            "endpoint-{service_instance:016x}-{generation:016x}-{}-{}",
            target.backend().name().to_ascii_lowercase(),
            target.numeric_index()
        ))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeviceEndpoint {
    key: DeviceEndpointKey,
    generation: u64,
    target: ProbeTarget,
    display_name: String,
}

impl DeviceEndpoint {
    pub(crate) fn from_available_outcome(
        key: DeviceEndpointKey,
        generation: u64,
        outcome: ProbeOutcome,
    ) -> Option<Self> {
        (outcome.status() == ProbeStatus::Available).then(|| {
            let target = outcome.target();
            Self {
                key,
                generation,
                target,
                display_name: format!(
                    "Camera {} / {}",
                    target.numeric_index(),
                    target.backend().name()
                ),
            }
        })
    }

    pub fn key(&self) -> &DeviceEndpointKey {
        &self.key
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn target(&self) -> ProbeTarget {
        self.target
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CameraFailureCode {
    OpenFailed,
    ReadTimeout,
    ReadStalled,
    Cancelled,
    Internal,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeviceScanSnapshot {
    pub(crate) operation_id: OperationId,
    pub(crate) scan_generation: u64,
    pub(crate) policy: DeviceScanPolicy,
    pub(crate) service_state: CameraServiceState,
    pub(crate) status: DeviceScanStatus,
    pub(crate) completed_probes: usize,
    pub(crate) total_probes: usize,
    pub(crate) current_probe: Option<ProbeTarget>,
    pub(crate) outcomes: Vec<ProbeOutcome>,
    pub(crate) endpoints: Vec<DeviceEndpoint>,
    pub(crate) failure_code: Option<CameraFailureCode>,
}

impl DeviceScanSnapshot {
    pub fn operation_id(&self) -> &OperationId {
        &self.operation_id
    }

    pub fn scan_generation(&self) -> u64 {
        self.scan_generation
    }

    pub fn policy(&self) -> &DeviceScanPolicy {
        &self.policy
    }

    pub fn service_state(&self) -> CameraServiceState {
        self.service_state
    }

    pub fn status(&self) -> DeviceScanStatus {
        self.status
    }

    pub fn completed_probes(&self) -> usize {
        self.completed_probes
    }

    pub fn total_probes(&self) -> usize {
        self.total_probes
    }

    pub fn current_probe(&self) -> Option<ProbeTarget> {
        self.current_probe
    }

    pub fn outcomes(&self) -> &[ProbeOutcome] {
        &self.outcomes
    }

    pub fn endpoints(&self) -> &[DeviceEndpoint] {
        &self.endpoints
    }

    pub fn failure_code(&self) -> Option<CameraFailureCode> {
        self.failure_code
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CameraServiceSnapshot {
    service_state: CameraServiceState,
    operation: Option<DeviceScanSnapshot>,
}

impl CameraServiceSnapshot {
    pub(crate) fn new(
        service_state: CameraServiceState,
        operation: Option<DeviceScanSnapshot>,
    ) -> Self {
        Self {
            service_state,
            operation,
        }
    }

    pub fn service_state(&self) -> CameraServiceState {
        self.service_state
    }

    pub fn operation(&self) -> Option<&DeviceScanSnapshot> {
        self.operation.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_builds_backend_major_cartesian_product() {
        let policy = DeviceScanPolicy::new(
            1,
            2,
            vec![CaptureBackend::Msmf, CaptureBackend::Dshow],
            10,
            100,
            20,
            1,
        )
        .expect("valid policy");

        assert_eq!(
            policy.targets(),
            vec![
                ProbeTarget::new(CaptureBackend::Msmf, 1),
                ProbeTarget::new(CaptureBackend::Msmf, 2),
                ProbeTarget::new(CaptureBackend::Dshow, 1),
                ProbeTarget::new(CaptureBackend::Dshow, 2),
            ]
        );
    }

    #[test]
    fn policy_rejects_range_probe_and_duration_boundaries() {
        let valid = |first, last, backends, first_ms, operation_ms, shutdown_ms, reopen_ms| {
            DeviceScanPolicy::new(
                first,
                last,
                backends,
                first_ms,
                operation_ms,
                shutdown_ms,
                reopen_ms,
            )
        };
        assert!(valid(0, 31, vec![CaptureBackend::Msmf], 1, 2, 1, 1).is_ok());
        assert!(valid(2, 1, vec![CaptureBackend::Msmf], 1, 2, 1, 1).is_err());
        assert!(valid(0, 32, vec![CaptureBackend::Msmf], 1, 2, 1, 1).is_err());
        assert!(valid(0, 1, Vec::new(), 1, 2, 1, 1).is_err());
        assert!(
            valid(
                0,
                1,
                vec![CaptureBackend::Msmf, CaptureBackend::Msmf],
                1,
                2,
                1,
                1
            )
            .is_err()
        );
        assert!(valid(0, 1, vec![CaptureBackend::Msmf], 0, 2, 1, 1).is_err());
        assert!(
            valid(
                0,
                1,
                vec![CaptureBackend::Msmf],
                1,
                MAX_DURATION_MS + 1,
                1,
                1
            )
            .is_err()
        );
        assert!(valid(0, 1, vec![CaptureBackend::Msmf], 2, 2, 1, 1).is_err());
        assert!(valid(0, 1, vec![CaptureBackend::Msmf], 1, 2, 3, 1).is_err());
    }

    #[test]
    fn endpoint_requires_an_available_outcome() {
        let target = ProbeTarget::new(CaptureBackend::Msmf, 0);
        let key = DeviceEndpointKey::new(1, 1, target);
        assert!(
            DeviceEndpoint::from_available_outcome(
                key.clone(),
                1,
                ProbeOutcome::new(target, ProbeStatus::OpenFailed),
            )
            .is_none()
        );
        assert!(
            DeviceEndpoint::from_available_outcome(
                key,
                1,
                ProbeOutcome::new(target, ProbeStatus::Available),
            )
            .is_some()
        );
    }

    #[test]
    fn operation_terminal_state_is_explicit() {
        assert!(!DeviceScanStatus::Scanning.is_terminal());
        for status in [
            DeviceScanStatus::Completed,
            DeviceScanStatus::Cancelled,
            DeviceScanStatus::Failed,
            DeviceScanStatus::Stuck,
        ] {
            assert!(status.is_terminal());
        }
    }
}
