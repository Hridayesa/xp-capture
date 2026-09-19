use serde::{Deserialize, Serialize};

use crate::camera::{
    CameraConfigError, CameraFailureCode, CameraService, CameraServiceError, CameraServiceSnapshot,
    CameraServiceState, CaptureBackend, DeviceEndpoint, DeviceScanPolicy, DeviceScanSnapshot,
    DeviceScanStatus, ProbeOutcome, ProbeStatus, ProbeTarget,
    model::{
        DEFAULT_FIRST_FRAME_DEADLINE_MS, DEFAULT_FIRST_INDEX, DEFAULT_LAST_INDEX,
        DEFAULT_OPERATION_DEADLINE_MS, DEFAULT_REOPEN_DELAY_MS, DEFAULT_SHUTDOWN_DEADLINE_MS,
    },
};

pub const CAMERA_TRANSPORT_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceScanRequestV1 {
    pub schema_version: u32,
    pub first_index: Option<u32>,
    pub last_index: Option<u32>,
    pub backends: Option<Vec<String>>,
    pub first_frame_deadline_ms: Option<u64>,
    pub operation_deadline_ms: Option<u64>,
    pub shutdown_deadline_ms: Option<u64>,
    pub reopen_delay_ms: Option<u64>,
}

impl DeviceScanRequestV1 {
    pub fn into_policy(self) -> Result<DeviceScanPolicy, CameraConfigError> {
        if self.schema_version != CAMERA_TRANSPORT_SCHEMA_VERSION {
            return Err(CameraConfigError::UnsupportedSchema);
        }
        let backends = self
            .backends
            .unwrap_or_else(|| vec!["MSMF".to_owned(), "DSHOW".to_owned()])
            .into_iter()
            .map(|backend| match backend.as_str() {
                "MSMF" => Ok(CaptureBackend::Msmf),
                "DSHOW" => Ok(CaptureBackend::Dshow),
                _ => Err(CameraConfigError::UnknownBackend),
            })
            .collect::<Result<Vec<_>, _>>()?;
        DeviceScanPolicy::new(
            self.first_index.unwrap_or(DEFAULT_FIRST_INDEX),
            self.last_index.unwrap_or(DEFAULT_LAST_INDEX),
            backends,
            self.first_frame_deadline_ms
                .unwrap_or(DEFAULT_FIRST_FRAME_DEADLINE_MS),
            self.operation_deadline_ms
                .unwrap_or(DEFAULT_OPERATION_DEADLINE_MS),
            self.shutdown_deadline_ms
                .unwrap_or(DEFAULT_SHUTDOWN_DEADLINE_MS),
            self.reopen_delay_ms.unwrap_or(DEFAULT_REOPEN_DELAY_MS),
        )
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceScanOperationRequestV1 {
    pub schema_version: u32,
    pub operation_id: String,
}

impl DeviceScanOperationRequestV1 {
    fn validate(self) -> Result<String, CameraConfigError> {
        if self.schema_version != CAMERA_TRANSPORT_SCHEMA_VERSION {
            return Err(CameraConfigError::UnsupportedSchema);
        }
        if self.operation_id.is_empty() {
            return Err(CameraConfigError::InvalidRequestShape);
        }
        Ok(self.operation_id)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceScanPolicyV1 {
    pub first_index: u32,
    pub last_index: u32,
    pub backends: Vec<String>,
    pub first_frame_deadline_ms: u64,
    pub operation_deadline_ms: u64,
    pub shutdown_deadline_ms: u64,
    pub reopen_delay_ms: u64,
}

impl From<&DeviceScanPolicy> for DeviceScanPolicyV1 {
    fn from(policy: &DeviceScanPolicy) -> Self {
        Self {
            first_index: policy.first_index(),
            last_index: policy.last_index(),
            backends: policy
                .backends()
                .iter()
                .map(|backend| backend.name().to_owned())
                .collect(),
            first_frame_deadline_ms: duration_ms(policy.first_frame_deadline()),
            operation_deadline_ms: duration_ms(policy.operation_deadline()),
            shutdown_deadline_ms: duration_ms(policy.shutdown_deadline()),
            reopen_delay_ms: duration_ms(policy.reopen_delay()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceScanStartedV1 {
    pub schema_version: u32,
    pub operation_id: String,
    pub scan_generation: u64,
    pub policy: DeviceScanPolicyV1,
}

impl From<&DeviceScanSnapshot> for DeviceScanStartedV1 {
    fn from(snapshot: &DeviceScanSnapshot) -> Self {
        Self {
            schema_version: CAMERA_TRANSPORT_SCHEMA_VERSION,
            operation_id: snapshot.operation_id().as_str().to_owned(),
            scan_generation: snapshot.scan_generation(),
            policy: DeviceScanPolicyV1::from(snapshot.policy()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeTargetV1 {
    pub backend: String,
    pub numeric_index: u32,
}

impl From<ProbeTarget> for ProbeTargetV1 {
    fn from(target: ProbeTarget) -> Self {
        Self {
            backend: target.backend().name().to_owned(),
            numeric_index: target.numeric_index(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeOutcomeV1 {
    pub backend: String,
    pub numeric_index: u32,
    pub status: String,
}

impl From<ProbeOutcome> for ProbeOutcomeV1 {
    fn from(outcome: ProbeOutcome) -> Self {
        let target = outcome.target();
        Self {
            backend: target.backend().name().to_owned(),
            numeric_index: target.numeric_index(),
            status: probe_status(outcome.status()).to_owned(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceEndpointV1 {
    pub endpoint_key: String,
    pub scan_generation: u64,
    pub backend: String,
    pub numeric_index: u32,
    pub display_name: String,
}

impl From<&DeviceEndpoint> for DeviceEndpointV1 {
    fn from(endpoint: &DeviceEndpoint) -> Self {
        let target = endpoint.target();
        Self {
            endpoint_key: endpoint.key().as_str().to_owned(),
            scan_generation: endpoint.generation(),
            backend: target.backend().name().to_owned(),
            numeric_index: target.numeric_index(),
            display_name: endpoint.display_name().to_owned(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceScanSnapshotV1 {
    pub schema_version: u32,
    pub operation_id: String,
    pub scan_generation: u64,
    pub policy: DeviceScanPolicyV1,
    pub service_state: String,
    pub status: String,
    pub completed_probes: usize,
    pub total_probes: usize,
    pub current_probe: Option<ProbeTargetV1>,
    pub outcomes: Vec<ProbeOutcomeV1>,
    pub endpoints: Vec<DeviceEndpointV1>,
    pub failure_code: Option<String>,
}

impl From<&DeviceScanSnapshot> for DeviceScanSnapshotV1 {
    fn from(snapshot: &DeviceScanSnapshot) -> Self {
        Self {
            schema_version: CAMERA_TRANSPORT_SCHEMA_VERSION,
            operation_id: snapshot.operation_id().as_str().to_owned(),
            scan_generation: snapshot.scan_generation(),
            policy: DeviceScanPolicyV1::from(snapshot.policy()),
            service_state: service_state(snapshot.service_state()).to_owned(),
            status: operation_status(snapshot.status()).to_owned(),
            completed_probes: snapshot.completed_probes(),
            total_probes: snapshot.total_probes(),
            current_probe: snapshot.current_probe().map(ProbeTargetV1::from),
            outcomes: snapshot
                .outcomes()
                .iter()
                .copied()
                .map(ProbeOutcomeV1::from)
                .collect(),
            endpoints: snapshot
                .endpoints()
                .iter()
                .map(DeviceEndpointV1::from)
                .collect(),
            failure_code: snapshot
                .failure_code()
                .map(public_failure_code)
                .map(str::to_owned),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CameraServiceSnapshotV1 {
    pub schema_version: u32,
    pub service_state: String,
    pub operation: Option<DeviceScanSnapshotV1>,
}

impl From<&CameraServiceSnapshot> for CameraServiceSnapshotV1 {
    fn from(snapshot: &CameraServiceSnapshot) -> Self {
        Self {
            schema_version: CAMERA_TRANSPORT_SCHEMA_VERSION,
            service_state: service_state(snapshot.service_state()).to_owned(),
            operation: snapshot.operation().map(DeviceScanSnapshotV1::from),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CameraPublicErrorV1 {
    pub schema_version: u32,
    pub code: String,
    pub message: String,
}

impl CameraPublicErrorV1 {
    pub fn invalid_config() -> Self {
        Self::new(
            "INVALID_CONFIG",
            "Параметры поиска камеры не прошли проверку.",
        )
    }

    fn new(code: &str, message: &str) -> Self {
        Self {
            schema_version: CAMERA_TRANSPORT_SCHEMA_VERSION,
            code: code.to_owned(),
            message: message.to_owned(),
        }
    }
}

impl From<&CameraServiceError> for CameraPublicErrorV1 {
    fn from(error: &CameraServiceError) -> Self {
        match error {
            CameraServiceError::InvalidConfig(_) => Self::invalid_config(),
            CameraServiceError::Busy => Self::new("BUSY", "Camera service уже выполняет scan."),
            CameraServiceError::Stuck => Self::new(
                "READ_STALLED",
                "Camera service требует перезапуска приложения.",
            ),
            CameraServiceError::StaleOperation => Self::new(
                "STALE_SCAN_OPERATION",
                "Запрошенная scan operation больше не является текущей.",
            ),
            CameraServiceError::Worker { source } => match source {
                crate::camera::CaptureAdapterError::Open { .. }
                | crate::camera::CaptureAdapterError::OpenState { .. } => {
                    Self::new("OPEN_FAILED", "Не удалось открыть camera endpoint.")
                }
                crate::camera::CaptureAdapterError::Read { .. } => {
                    Self::new("READ_TIMEOUT", "Не удалось получить camera frame.")
                }
                crate::camera::CaptureAdapterError::Release { .. } => {
                    Self::new("INTERNAL", "Внутренняя ошибка camera service.")
                }
            },
            CameraServiceError::GenerationExhausted
            | CameraServiceError::WorkerStart { .. }
            | CameraServiceError::Synchronization
            | CameraServiceError::WorkerPanicked => {
                Self::new("INTERNAL", "Внутренняя ошибка camera service.")
            }
        }
    }
}

pub fn start_device_scan_for(
    service: &CameraService,
    request: serde_json::Value,
) -> Result<DeviceScanStartedV1, CameraPublicErrorV1> {
    let request = decode_scan_request(request)?;
    let policy = request
        .into_policy()
        .map_err(|_| CameraPublicErrorV1::invalid_config())?;
    let snapshot = service.start(policy).map_err(map_service_error)?;
    Ok(DeviceScanStartedV1::from(&snapshot))
}

pub fn get_device_scan_for(
    service: &CameraService,
    request: serde_json::Value,
) -> Result<DeviceScanSnapshotV1, CameraPublicErrorV1> {
    let operation_id = decode_operation_request(request)?;
    let snapshot = service.get(&operation_id).map_err(map_service_error)?;
    Ok(DeviceScanSnapshotV1::from(&snapshot))
}

pub fn cancel_device_scan_for(
    service: &CameraService,
    request: serde_json::Value,
) -> Result<DeviceScanSnapshotV1, CameraPublicErrorV1> {
    let operation_id = decode_operation_request(request)?;
    let snapshot = service.cancel(&operation_id).map_err(map_service_error)?;
    Ok(DeviceScanSnapshotV1::from(&snapshot))
}

pub fn stop_camera_for(
    service: &CameraService,
) -> Result<CameraServiceSnapshotV1, CameraPublicErrorV1> {
    let snapshot = service.stop().map_err(map_service_error)?;
    Ok(CameraServiceSnapshotV1::from(&snapshot))
}

fn decode_scan_request(
    request: serde_json::Value,
) -> Result<DeviceScanRequestV1, CameraPublicErrorV1> {
    serde_json::from_value(request).map_err(|_| CameraPublicErrorV1::invalid_config())
}

fn decode_operation_request(request: serde_json::Value) -> Result<String, CameraPublicErrorV1> {
    serde_json::from_value::<DeviceScanOperationRequestV1>(request)
        .map_err(|_| CameraPublicErrorV1::invalid_config())?
        .validate()
        .map_err(|_| CameraPublicErrorV1::invalid_config())
}

fn map_service_error(error: CameraServiceError) -> CameraPublicErrorV1 {
    CameraPublicErrorV1::from(&error)
}

fn duration_ms(duration: std::time::Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn service_state(state: CameraServiceState) -> &'static str {
    match state {
        CameraServiceState::Idle => "idle",
        CameraServiceState::Scanning => "scanning",
        CameraServiceState::Faulted => "faulted",
        CameraServiceState::Stuck => "stuck",
    }
}

fn operation_status(status: DeviceScanStatus) -> &'static str {
    match status {
        DeviceScanStatus::Scanning => "scanning",
        DeviceScanStatus::Completed => "completed",
        DeviceScanStatus::Cancelled => "cancelled",
        DeviceScanStatus::Failed => "failed",
        DeviceScanStatus::Stuck => "stuck",
    }
}

fn probe_status(status: ProbeStatus) -> &'static str {
    match status {
        ProbeStatus::Available => "available",
        ProbeStatus::OpenFailed => "open_failed",
        ProbeStatus::FirstFrameTimeout => "first_frame_timeout",
        ProbeStatus::ReadFailed => "read_failed",
        ProbeStatus::Cancelled => "cancelled",
    }
}

fn public_failure_code(code: CameraFailureCode) -> &'static str {
    match code {
        CameraFailureCode::OpenFailed => "OPEN_FAILED",
        CameraFailureCode::ReadTimeout => "READ_TIMEOUT",
        CameraFailureCode::ReadStalled => "READ_STALLED",
        CameraFailureCode::Cancelled => "CANCELLED",
        CameraFailureCode::Internal => "INTERNAL",
    }
}

#[cfg(test)]
mod tests {
    use std::{error::Error, io, sync::Arc};

    use super::*;
    use crate::camera::{
        CaptureAdapter, CaptureAdapterError, CaptureAdapterFactory, CaptureAdapterResult,
        CaptureOpen, ProbeTarget,
    };

    struct UnavailableFactory;

    impl CaptureAdapterFactory for UnavailableFactory {
        fn create(&self) -> Box<dyn CaptureAdapter> {
            Box::new(UnavailableAdapter)
        }
    }

    struct UnavailableAdapter;

    impl CaptureAdapter for UnavailableAdapter {
        fn open(&mut self, _target: ProbeTarget) -> CaptureAdapterResult<CaptureOpen> {
            Ok(CaptureOpen::Unavailable)
        }
    }

    fn default_request() -> DeviceScanRequestV1 {
        DeviceScanRequestV1 {
            schema_version: 1,
            first_index: None,
            last_index: None,
            backends: None,
            first_frame_deadline_ms: None,
            operation_deadline_ms: None,
            shutdown_deadline_ms: None,
            reopen_delay_ms: None,
        }
    }

    #[test]
    fn request_defaults_are_observable_and_ordered() {
        let policy = default_request().into_policy().expect("defaults are valid");
        assert_eq!((policy.first_index(), policy.last_index()), (0, 5));
        assert_eq!(
            policy.backends(),
            &[CaptureBackend::Msmf, CaptureBackend::Dshow]
        );
    }

    #[test]
    fn request_rejects_unknown_schema_backend_and_shape() {
        let mut unknown_schema = default_request();
        unknown_schema.schema_version = 2;
        assert_eq!(
            unknown_schema.into_policy(),
            Err(CameraConfigError::UnsupportedSchema)
        );

        let mut unknown_backend = default_request();
        unknown_backend.backends = Some(vec!["CAP_ANY".to_owned()]);
        assert_eq!(
            unknown_backend.into_policy(),
            Err(CameraConfigError::UnknownBackend)
        );

        let value = serde_json::json!({"schema_version": 1, "unknown": true});
        let error = decode_scan_request(value).expect_err("unknown field must be rejected");
        assert_eq!(error.code, "INVALID_CONFIG");
    }

    #[test]
    fn invalid_request_does_not_consume_scan_generation() {
        let service = CameraService::new(Arc::new(UnavailableFactory));
        let invalid = serde_json::json!({
            "schema_version": 1,
            "first_index": 2,
            "last_index": 1
        });
        assert_eq!(
            start_device_scan_for(&service, invalid)
                .expect_err("invalid range is rejected")
                .code,
            "INVALID_CONFIG"
        );

        let accepted = start_device_scan_for(
            &service,
            serde_json::json!({
                "schema_version": 1,
                "first_index": 0,
                "last_index": 0,
                "backends": ["MSMF"],
                "first_frame_deadline_ms": 10,
                "operation_deadline_ms": 100,
                "shutdown_deadline_ms": 10,
                "reopen_delay_ms": 1
            }),
        )
        .expect("valid request starts");
        assert_eq!(accepted.scan_generation, 1);
        let _ = service.stop();
    }

    #[test]
    fn public_mapper_never_serializes_internal_source() {
        let marker = r"C:\private\NATIVE_MARKER.dll";
        let source = CaptureAdapterError::read(io::Error::other(marker));
        assert_eq!(
            source.source().map(ToString::to_string).as_deref(),
            Some(marker)
        );
        let error = CameraServiceError::Worker { source };
        let public = CameraPublicErrorV1::from(&error);
        let json = serde_json::to_string(&public).expect("public error serializes");
        assert_eq!(public.code, "READ_TIMEOUT");
        assert!(!json.contains(marker));
        assert!(!json.contains("source"));
    }

    #[test]
    fn dto_round_trip_rejects_unknown_fields() {
        let started = DeviceScanStartedV1 {
            schema_version: 1,
            operation_id: "opaque".to_owned(),
            scan_generation: 1,
            policy: DeviceScanPolicyV1::from(&DeviceScanPolicy::defaults()),
        };
        let json = serde_json::to_value(&started).expect("started DTO serializes");
        assert_eq!(
            serde_json::from_value::<DeviceScanStartedV1>(json).expect("DTO round-trips"),
            started
        );
        assert!(
            serde_json::from_value::<DeviceScanOperationRequestV1>(serde_json::json!({
                "schema_version": 1,
                "operation_id": "opaque",
                "path": "C:\\private"
            }))
            .is_err()
        );
    }
}
