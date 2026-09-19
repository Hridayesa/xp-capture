use serde::{Deserialize, Serialize};

use crate::camera::{
    CameraConfigError, CameraFailureCode, CameraService, CameraServiceError, CameraServiceSnapshot,
    CameraServiceState, CaptureBackend, DeviceEndpoint, DeviceScanPolicy, DeviceScanSnapshot,
    DeviceScanStatus, ProbeOutcome, ProbeStatus, ProbeTarget,
    model::{
        DEFAULT_FIRST_FRAME_DEADLINE_MS, DEFAULT_FIRST_INDEX, DEFAULT_LAST_INDEX,
        DEFAULT_OPERATION_DEADLINE_MS, DEFAULT_REOPEN_DELAY_MS, DEFAULT_SHUTDOWN_DEADLINE_MS,
    },
    profiling::{
        CandidateFailureReason, CandidatePhase, CandidateResult, CaptureMetrics, CaptureModeStatus,
        ModeCandidatePolicyV1, ModeTuple, ProfileOperationStatus, ProfileReport, ProfileSnapshot,
        ReportedCaptureProperties, Resolution, ResolutionMaximum,
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

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StartProfileRequestV1 {
    pub schema_version: u32,
    pub endpoint_key: String,
    pub candidate_config: ModeCandidatePolicyV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileOperationRequestV1 {
    pub schema_version: u32,
    pub profile_id: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModeTupleV1 {
    pub fourcc: String,
    pub width: u32,
    pub height: u32,
    pub requested_fps: f64,
}

impl From<ModeTuple> for ModeTupleV1 {
    fn from(value: ModeTuple) -> Self {
        Self {
            fourcc: value.fourcc.to_string(),
            width: value.resolution.width,
            height: value.resolution.height,
            requested_fps: value.requested_fps,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileStartedV1 {
    pub schema_version: u32,
    pub profile_id: String,
    pub endpoint_key: String,
    pub scan_generation: u64,
    pub backend: String,
    pub policy: ModeCandidatePolicyV1,
    pub config_hash: String,
    pub total_candidates: usize,
}

impl From<&ProfileSnapshot> for ProfileStartedV1 {
    fn from(snapshot: &ProfileSnapshot) -> Self {
        Self {
            schema_version: CAMERA_TRANSPORT_SCHEMA_VERSION,
            profile_id: snapshot.profile_id.as_str().to_owned(),
            endpoint_key: snapshot.endpoint_key.as_str().to_owned(),
            scan_generation: snapshot.scan_generation,
            backend: snapshot.backend.name().to_owned(),
            policy: snapshot.policy.clone(),
            config_hash: snapshot.config_hash.clone(),
            total_candidates: snapshot.total_candidates,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileProgressV1 {
    pub schema_version: u32,
    pub profile_id: String,
    pub service_state: String,
    pub status: String,
    pub endpoint_key: String,
    pub scan_generation: u64,
    pub backend: String,
    pub policy: ModeCandidatePolicyV1,
    pub config_hash: String,
    pub completed_candidates: usize,
    pub total_candidates: usize,
    pub current_candidate: Option<ModeTupleV1>,
    pub current_phase: Option<String>,
    pub failure_code: Option<String>,
}

impl From<&ProfileSnapshot> for ProfileProgressV1 {
    fn from(snapshot: &ProfileSnapshot) -> Self {
        Self {
            schema_version: CAMERA_TRANSPORT_SCHEMA_VERSION,
            profile_id: snapshot.profile_id.as_str().to_owned(),
            service_state: profile_service_state(snapshot.service_state).to_owned(),
            status: profile_status(snapshot.status).to_owned(),
            endpoint_key: snapshot.endpoint_key.as_str().to_owned(),
            scan_generation: snapshot.scan_generation,
            backend: snapshot.backend.name().to_owned(),
            policy: snapshot.policy.clone(),
            config_hash: snapshot.config_hash.clone(),
            completed_candidates: snapshot.completed_candidates,
            total_candidates: snapshot.total_candidates,
            current_candidate: snapshot.current_candidate.map(ModeTupleV1::from),
            current_phase: snapshot.current_phase.map(profile_phase).map(str::to_owned),
            failure_code: snapshot
                .failure_reason
                .map(profile_failure_reason)
                .map(str::to_owned),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PropertySetDiagnosticsV1 {
    pub fourcc: bool,
    pub width: bool,
    pub height: bool,
    pub fps: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReportedCapturePropertiesV1 {
    pub fourcc: Option<String>,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub fps: Option<f64>,
}

impl From<ReportedCaptureProperties> for ReportedCapturePropertiesV1 {
    fn from(value: ReportedCaptureProperties) -> Self {
        Self {
            fourcc: value.fourcc.map(|fourcc| fourcc.to_string()),
            width: value.width,
            height: value.height,
            fps: value.fps,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureMetricsV1 {
    pub elapsed_ms: f64,
    pub read_attempts: u64,
    pub captured_frames: u64,
    pub empty_frames: u64,
    pub read_failures: u64,
    pub measured_fps: f64,
    pub median_interval_ms: Option<f64>,
    pub p95_interval_ms: Option<f64>,
    pub p99_interval_ms: Option<f64>,
    pub maximum_gap_ms: Option<f64>,
    pub long_gap_count: u64,
    pub long_gap_ratio: f64,
    pub actual_resolutions: Vec<ResolutionV1>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolutionV1 {
    pub width: u32,
    pub height: u32,
}

impl From<Resolution> for ResolutionV1 {
    fn from(value: Resolution) -> Self {
        Self {
            width: value.width,
            height: value.height,
        }
    }
}

impl From<&CaptureMetrics> for CaptureMetricsV1 {
    fn from(value: &CaptureMetrics) -> Self {
        Self {
            elapsed_ms: value.elapsed.as_secs_f64() * 1_000.0,
            read_attempts: value.read_attempts,
            captured_frames: value.captured_frames,
            empty_frames: value.empty_frames,
            read_failures: value.read_failures,
            measured_fps: value.measured_fps,
            median_interval_ms: duration_ms_f64(value.median_interval),
            p95_interval_ms: duration_ms_f64(value.p95_interval),
            p99_interval_ms: duration_ms_f64(value.p99_interval),
            maximum_gap_ms: duration_ms_f64(value.maximum_gap),
            long_gap_count: value.long_gap_count,
            long_gap_ratio: value.long_gap_ratio,
            actual_resolutions: value
                .actual_resolutions
                .iter()
                .copied()
                .map(ResolutionV1::from)
                .collect(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateResultV1 {
    pub ordinal: usize,
    pub attempt_count: u8,
    pub retry_reason: Option<String>,
    pub requested: ModeTupleV1,
    pub set: PropertySetDiagnosticsV1,
    pub reported: ReportedCapturePropertiesV1,
    pub metrics: CaptureMetricsV1,
    pub capture_mode_status: String,
    pub failure_reasons: Vec<String>,
    pub verified_mode_id: Option<String>,
}

impl From<&CandidateResult> for CandidateResultV1 {
    fn from(value: &CandidateResult) -> Self {
        Self {
            ordinal: value.ordinal,
            attempt_count: value.attempt_count,
            retry_reason: value.retry_reason.map(|reason| {
                match reason {
                    crate::camera::profiling::CandidateRetryReason::OpenFailed => "open_failed",
                    crate::camera::profiling::CandidateRetryReason::FirstReadStartFailed => {
                        "first_read_start_failed"
                    }
                }
                .to_owned()
            }),
            requested: ModeTupleV1::from(value.tuple),
            set: PropertySetDiagnosticsV1 {
                fourcc: value.set_diagnostics.fourcc,
                width: value.set_diagnostics.width,
                height: value.set_diagnostics.height,
                fps: value.set_diagnostics.fps,
            },
            reported: ReportedCapturePropertiesV1::from(value.reported),
            metrics: CaptureMetricsV1::from(&value.metrics),
            capture_mode_status: capture_mode_status(value.status).to_owned(),
            failure_reasons: value
                .failure_reasons
                .iter()
                .copied()
                .map(profile_failure_reason)
                .map(str::to_owned)
                .collect(),
            verified_mode_id: value
                .verified_mode_id()
                .map(|identifier| identifier.as_str().to_owned()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolutionMaximumV1 {
    pub resolution: ResolutionV1,
    pub result_ordinals: Vec<usize>,
    pub measured_fps: f64,
}

impl From<&ResolutionMaximum> for ResolutionMaximumV1 {
    fn from(value: &ResolutionMaximum) -> Self {
        Self {
            resolution: ResolutionV1::from(value.resolution),
            result_ordinals: value.result_ordinals.clone(),
            measured_fps: value.measured_fps,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentVersionReferencesV1 {
    pub app_version: String,
    pub rust_version: String,
    pub tauri_version: String,
    pub opencv_version: String,
    pub opencv_crate_version: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileReportV1 {
    pub schema_version: u32,
    pub profile_id: String,
    pub started_at_unix_ms: u64,
    pub environment: EnvironmentVersionReferencesV1,
    pub endpoint_key: String,
    pub scan_generation: u64,
    pub backend: String,
    pub policy: ModeCandidatePolicyV1,
    pub config_hash: String,
    pub status: String,
    pub failure_code: Option<String>,
    pub runtime_validation_status: String,
    pub external_validation_status: String,
    pub results: Vec<CandidateResultV1>,
    pub max_verified_by_resolution: Vec<ResolutionMaximumV1>,
}

impl From<&ProfileReport> for ProfileReportV1 {
    fn from(value: &ProfileReport) -> Self {
        Self {
            schema_version: CAMERA_TRANSPORT_SCHEMA_VERSION,
            profile_id: value.profile_id.as_str().to_owned(),
            started_at_unix_ms: value.started_at_unix_ms,
            environment: EnvironmentVersionReferencesV1 {
                app_version: value.environment.app_version.clone(),
                rust_version: value.environment.rust_version.clone(),
                tauri_version: value.environment.tauri_version.clone(),
                opencv_version: value.environment.opencv_version.clone(),
                opencv_crate_version: value.environment.opencv_crate_version.clone(),
            },
            endpoint_key: value.endpoint_key.as_str().to_owned(),
            scan_generation: value.scan_generation,
            backend: value.backend.name().to_owned(),
            policy: value.policy.clone(),
            config_hash: value.config_hash.clone(),
            status: profile_terminal_status(value.status).to_owned(),
            failure_code: value
                .failure_reason
                .map(profile_failure_reason)
                .map(str::to_owned),
            runtime_validation_status: "not_run".to_owned(),
            external_validation_status: "not_run".to_owned(),
            results: value.results.iter().map(CandidateResultV1::from).collect(),
            max_verified_by_resolution: value
                .max_verified_by_resolution
                .iter()
                .map(ResolutionMaximumV1::from)
                .collect(),
        }
    }
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
            CameraServiceError::InvalidConfig(_) | CameraServiceError::InvalidProfileConfig(_) => {
                Self::invalid_config()
            }
            CameraServiceError::Busy => Self::new("BUSY", "Camera service уже выполняет scan."),
            CameraServiceError::Stuck => Self::new(
                "READ_STALLED",
                "Camera service требует перезапуска приложения.",
            ),
            CameraServiceError::StaleOperation => Self::new(
                "STALE_SCAN_OPERATION",
                "Запрошенная scan operation больше не является текущей.",
            ),
            CameraServiceError::StaleDeviceEndpoint => Self::new(
                "STALE_DEVICE_ENDPOINT",
                "Выбранный camera endpoint больше не является актуальным.",
            ),
            CameraServiceError::StaleProfileOperation => Self::new(
                "STALE_PROFILE_OPERATION",
                "Запрошенная profile operation больше не является текущей.",
            ),
            CameraServiceError::ProfileNotReady => {
                Self::new("PROFILE_NOT_READY", "Результат profiling ещё не готов.")
            }
            CameraServiceError::Worker { source } => match source {
                crate::camera::CaptureAdapterError::Open { .. }
                | crate::camera::CaptureAdapterError::OpenState { .. } => {
                    Self::new("OPEN_FAILED", "Не удалось открыть camera endpoint.")
                }
                crate::camera::CaptureAdapterError::Read { .. } => {
                    Self::new("READ_TIMEOUT", "Не удалось получить camera frame.")
                }
                crate::camera::CaptureAdapterError::Apply { .. }
                | crate::camera::CaptureAdapterError::Get { .. }
                | crate::camera::CaptureAdapterError::Release { .. } => {
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

pub fn start_profile_for(
    service: &CameraService,
    request: serde_json::Value,
) -> Result<ProfileStartedV1, CameraPublicErrorV1> {
    let request: StartProfileRequestV1 =
        serde_json::from_value(request).map_err(|_| CameraPublicErrorV1::invalid_config())?;
    if request.schema_version != CAMERA_TRANSPORT_SCHEMA_VERSION || request.endpoint_key.is_empty()
    {
        return Err(CameraPublicErrorV1::invalid_config());
    }
    let snapshot = service
        .start_profile(&request.endpoint_key, request.candidate_config)
        .map_err(map_service_error)?;
    Ok(ProfileStartedV1::from(&snapshot))
}

pub fn get_profile_status_for(
    service: &CameraService,
    request: serde_json::Value,
) -> Result<ProfileProgressV1, CameraPublicErrorV1> {
    let profile_id = decode_profile_operation_request(request)?;
    let snapshot = service
        .get_profile_status(&profile_id)
        .map_err(map_service_error)?;
    Ok(ProfileProgressV1::from(&snapshot))
}

pub fn get_profile_result_for(
    service: &CameraService,
    request: serde_json::Value,
) -> Result<ProfileReportV1, CameraPublicErrorV1> {
    let profile_id = decode_profile_operation_request(request)?;
    let report = service
        .get_profile_result(&profile_id)
        .map_err(map_service_error)?;
    Ok(ProfileReportV1::from(&report))
}

pub fn cancel_profile_for(
    service: &CameraService,
    request: serde_json::Value,
) -> Result<ProfileProgressV1, CameraPublicErrorV1> {
    let profile_id = decode_profile_operation_request(request)?;
    let snapshot = service
        .cancel_profile(&profile_id)
        .map_err(map_service_error)?;
    Ok(ProfileProgressV1::from(&snapshot))
}

fn decode_profile_operation_request(
    request: serde_json::Value,
) -> Result<String, CameraPublicErrorV1> {
    let request: ProfileOperationRequestV1 =
        serde_json::from_value(request).map_err(|_| CameraPublicErrorV1::invalid_config())?;
    if request.schema_version != CAMERA_TRANSPORT_SCHEMA_VERSION || request.profile_id.is_empty() {
        return Err(CameraPublicErrorV1::invalid_config());
    }
    Ok(request.profile_id)
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

fn duration_ms_f64(duration: Option<std::time::Duration>) -> Option<f64> {
    duration.map(|value| value.as_secs_f64() * 1_000.0)
}

fn profile_service_state(state: CameraServiceState) -> &'static str {
    match state {
        CameraServiceState::Idle => "idle",
        CameraServiceState::Scanning => "scanning",
        CameraServiceState::Profiling => "profiling",
        CameraServiceState::ProfileReady => "profile_ready",
        CameraServiceState::Faulted => "faulted",
        CameraServiceState::Stuck => "stuck",
    }
}

fn profile_status(status: ProfileOperationStatus) -> &'static str {
    match status {
        ProfileOperationStatus::Profiling => "profiling",
        ProfileOperationStatus::Completed => "completed",
        ProfileOperationStatus::Cancelled => "cancelled",
        ProfileOperationStatus::Failed => "failed",
        ProfileOperationStatus::Stuck => "stuck",
    }
}

fn profile_terminal_status(
    status: crate::camera::profiling::ProfileTerminalStatus,
) -> &'static str {
    match status {
        crate::camera::profiling::ProfileTerminalStatus::Completed => "completed",
        crate::camera::profiling::ProfileTerminalStatus::Cancelled => "cancelled",
        crate::camera::profiling::ProfileTerminalStatus::Failed => "failed",
        crate::camera::profiling::ProfileTerminalStatus::Stuck => "stuck",
    }
}

fn profile_phase(phase: CandidatePhase) -> &'static str {
    match phase {
        CandidatePhase::Opening => "opening",
        CandidatePhase::ApplyingProperties => "applying",
        CandidatePhase::ReadingReportedProperties => "reported",
        CandidatePhase::FirstFrame => "first_frame",
        CandidatePhase::Warmup => "warmup",
        CandidatePhase::Measuring => "measuring",
        CandidatePhase::Release => "release",
        CandidatePhase::ReopenDelay => "reopen_delay",
    }
}

fn capture_mode_status(status: CaptureModeStatus) -> &'static str {
    match status {
        CaptureModeStatus::OpeningFailed => "opening_failed",
        CaptureModeStatus::ApplyingFailed => "applying_failed",
        CaptureModeStatus::ReadFailed => "read_failed",
        CaptureModeStatus::ReleaseFailed => "release_failed",
        CaptureModeStatus::FirstFrameTimeout => "first_frame_timeout",
        CaptureModeStatus::ReadStalled => "read_stalled",
        CaptureModeStatus::CandidateTimedOut => "candidate_timed_out",
        CaptureModeStatus::OperationTimedOut => "operation_timed_out",
        CaptureModeStatus::CoercedResolution => "coerced_resolution",
        CaptureModeStatus::CaptureUnderTarget => "capture_under_target",
        CaptureModeStatus::CaptureUnstable => "capture_unstable",
        CaptureModeStatus::VerifiedFourCcReportedMatch => "verified_fourcc_reported_match",
        CaptureModeStatus::VerifiedFourCcUnconfirmed => "verified_fourcc_unconfirmed",
        CaptureModeStatus::Cancelled => "cancelled",
    }
}

fn profile_failure_reason(reason: CandidateFailureReason) -> &'static str {
    match reason {
        CandidateFailureReason::OpenFailed => "OPEN_FAILED",
        CandidateFailureReason::ApplyFailed => "APPLY_FAILED",
        CandidateFailureReason::ReadFailed => "READ_FAILED",
        CandidateFailureReason::ReleaseFailed => "RELEASE_FAILED",
        CandidateFailureReason::FirstFrameTimeout => "FIRST_FRAME_TIMEOUT",
        CandidateFailureReason::ReadStalled => "READ_STALLED",
        CandidateFailureReason::CandidateDeadline => "CANDIDATE_TIMEOUT",
        CandidateFailureReason::OperationDeadline => "READ_TIMEOUT",
        CandidateFailureReason::Cancelled => "CANCELLED",
        CandidateFailureReason::InsufficientFrames => "INSUFFICIENT_FRAMES",
        CandidateFailureReason::CoercedResolution => "MODE_COERCED",
        CandidateFailureReason::UnderTargetFps => "UNDER_TARGET_FPS",
        CandidateFailureReason::ReadFailureRatio => "READ_FAILURE_RATIO",
        CandidateFailureReason::MaximumGap => "MAXIMUM_GAP",
        CandidateFailureReason::LongGapRatio => "LONG_GAP_RATIO",
    }
}

fn service_state(state: CameraServiceState) -> &'static str {
    match state {
        CameraServiceState::Idle => "idle",
        CameraServiceState::Scanning => "scanning",
        CameraServiceState::Profiling => "scanning",
        CameraServiceState::ProfileReady => "idle",
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
        for source in [
            CaptureAdapterError::apply(io::Error::other(marker)),
            CaptureAdapterError::get(io::Error::other(marker)),
            CaptureAdapterError::read(io::Error::other(marker)),
            CaptureAdapterError::release(io::Error::other(marker)),
        ] {
            assert_eq!(
                source.source().map(ToString::to_string).as_deref(),
                Some(marker)
            );
            let error = CameraServiceError::Worker { source };
            let public = CameraPublicErrorV1::from(&error);
            let json = serde_json::to_string(&public).expect("public error serializes");
            assert!(!json.contains(marker));
            assert!(!json.contains("source"));
        }
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

    #[test]
    fn profile_requests_reject_unknown_fields_and_map_stale_not_ready_codes() {
        let malformed = serde_json::json!({
            "schema_version": 1,
            "profile_id": "profile-1",
            "native_path": "C:\\private"
        });
        assert_eq!(
            decode_profile_operation_request(malformed)
                .expect_err("extended request is rejected")
                .code,
            "INVALID_CONFIG"
        );
        assert_eq!(
            CameraPublicErrorV1::from(&CameraServiceError::StaleDeviceEndpoint).code,
            "STALE_DEVICE_ENDPOINT"
        );
        assert_eq!(
            CameraPublicErrorV1::from(&CameraServiceError::StaleProfileOperation).code,
            "STALE_PROFILE_OPERATION"
        );
        assert_eq!(
            CameraPublicErrorV1::from(&CameraServiceError::ProfileNotReady).code,
            "PROFILE_NOT_READY"
        );
    }

    #[test]
    fn profile_report_dto_is_versioned_compact_and_contains_no_internal_fields() {
        let policy = crate::camera::profiling::ModeCandidatePolicy::from_json(include_str!(
            "../../../config/mode-candidates.json"
        ))
        .expect("default policy");
        let report = crate::camera::profiling::ProfileReport::finalize(
            crate::camera::profiling::ProfileReportInput {
                profile_id: crate::camera::profiling::ProfileId::new("profile-1"),
                started_at_unix_ms: 1,
                environment: crate::camera::profiling::EnvironmentVersionReferences {
                    app_version: "0.1.0".into(),
                    rust_version: "1.98.1".into(),
                    tauri_version: "2.11.5".into(),
                    opencv_version: "4.12.0".into(),
                    opencv_crate_version: "0.100.1".into(),
                },
                endpoint_key: crate::camera::DeviceEndpointKey("endpoint-1".into()),
                scan_generation: 1,
                backend: CaptureBackend::Dshow,
                policy,
                status: crate::camera::profiling::ProfileTerminalStatus::Completed,
                failure_reason: None,
                results: Vec::new(),
            },
        )
        .expect("report");
        let dto = ProfileReportV1::from(&report);
        let json = serde_json::to_string(&dto).expect("serialize report DTO");
        assert_eq!(dto.schema_version, 1);
        assert!(json.contains("\"runtime_validation_status\":\"not_run\""));
        for forbidden in ["source", "path", "frame_bytes", "native_path"] {
            assert!(!json.contains(forbidden));
        }
        let decoded: ProfileReportV1 = serde_json::from_str(&json).expect("strict round trip");
        assert_eq!(decoded, dto);
    }
}
