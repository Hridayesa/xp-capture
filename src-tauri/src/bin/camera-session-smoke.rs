use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use serde::Serialize;
use thiserror::Error;
use xp_capture_lib::camera::{
    CameraService, CameraServiceError, CaptureBackend, DeviceScanPolicy, DeviceScanPolicyV1,
    DeviceScanSnapshot, DeviceScanSnapshotV1, DeviceScanStatus, OpenCvCaptureAdapterFactory,
};

const EVIDENCE_SCHEMA_VERSION: u32 = 1;
const POLL_INTERVAL: Duration = Duration::from_millis(25);

#[derive(Debug, Error)]
enum SmokeError {
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    #[error("camera service failed")]
    Camera(#[from] CameraServiceError),
    #[error("OpenCV version query failed")]
    OpenCv(#[from] opencv::Error),
    #[error("evidence serialization failed")]
    Serialize(#[from] serde_json::Error),
    #[error("evidence directory creation failed")]
    CreateDirectory(#[source] std::io::Error),
    #[error("evidence write failed")]
    Write(#[source] std::io::Error),
    #[error("camera smoke did not satisfy its acceptance gate: {0}")]
    Acceptance(&'static str),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Options {
    first_index: u32,
    last_index: u32,
    backends: Vec<CaptureBackend>,
    evidence_path: PathBuf,
    hardware_context: HardwareContext,
    first_frame_deadline_ms: u64,
    operation_deadline_ms: u64,
    shutdown_deadline_ms: u64,
    reopen_delay_ms: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum HardwareContext {
    CameraPresent,
    CameraLessHost,
}

#[derive(Serialize)]
struct CameraSmokeEvidence {
    schema_version: u32,
    evidence_kind: &'static str,
    policy: DeviceScanPolicyV1,
    runs: Vec<DeviceScanSnapshotV1>,
    release_reopen: ReleaseReopenEvidence,
    hardware_context: HardwareContext,
    no_camera_validation: NoCameraEvidence,
    environment_versions: EnvironmentVersions,
}

#[derive(Serialize)]
struct ReleaseReopenEvidence {
    first_scan_completed: bool,
    second_scan_completed: bool,
    generation_incremented: bool,
    first_scan_found_endpoint: bool,
    second_scan_found_endpoint: bool,
}

#[derive(Serialize)]
struct NoCameraEvidence {
    real_scan_completed_empty: bool,
    repeated_scan_completed_empty: bool,
    scripted_test_reference: &'static str,
}

#[derive(Serialize)]
struct EnvironmentVersions {
    application: &'static str,
    opencv: String,
    opencv_rust_crate: &'static str,
    target_os: &'static str,
    target_arch: &'static str,
}

fn main() -> std::process::ExitCode {
    match run(std::env::args().skip(1)) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("camera_session_smoke failed: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run(arguments: impl IntoIterator<Item = String>) -> Result<(), SmokeError> {
    let options = parse_options(arguments)?;
    let policy = DeviceScanPolicy::new(
        options.first_index,
        options.last_index,
        options.backends,
        options.first_frame_deadline_ms,
        options.operation_deadline_ms,
        options.shutdown_deadline_ms,
        options.reopen_delay_ms,
    )?;
    let service = CameraService::new(Arc::new(OpenCvCaptureAdapterFactory::new()));
    let first = run_scan(&service, policy.clone())?;
    let second = run_scan(&service, policy.clone())?;
    let _ = service.stop()?;

    let release_reopen = ReleaseReopenEvidence {
        first_scan_completed: first.status() == DeviceScanStatus::Completed,
        second_scan_completed: second.status() == DeviceScanStatus::Completed,
        generation_incremented: second.scan_generation() > first.scan_generation(),
        first_scan_found_endpoint: !first.endpoints().is_empty(),
        second_scan_found_endpoint: !second.endpoints().is_empty(),
    };
    let evidence = CameraSmokeEvidence {
        schema_version: EVIDENCE_SCHEMA_VERSION,
        evidence_kind: "opencv-camera-session-smoke",
        policy: DeviceScanPolicyV1::from(&policy),
        runs: vec![
            DeviceScanSnapshotV1::from(&first),
            DeviceScanSnapshotV1::from(&second),
        ],
        release_reopen,
        hardware_context: options.hardware_context,
        no_camera_validation: NoCameraEvidence {
            real_scan_completed_empty: first.status() == DeviceScanStatus::Completed
                && first.endpoints().is_empty(),
            repeated_scan_completed_empty: second.status() == DeviceScanStatus::Completed
                && second.endpoints().is_empty(),
            scripted_test_reference: "camera::service::tests::no_camera_is_a_successful_empty_result",
        },
        environment_versions: EnvironmentVersions {
            application: env!("CARGO_PKG_VERSION"),
            opencv: opencv::core::get_version_string()?,
            opencv_rust_crate: "0.100.1",
            target_os: std::env::consts::OS,
            target_arch: std::env::consts::ARCH,
        },
    };
    write_evidence(&options.evidence_path, &evidence)?;

    if first.status() != DeviceScanStatus::Completed
        || second.status() != DeviceScanStatus::Completed
    {
        return Err(SmokeError::Acceptance(
            "both bounded scans must complete without a service failure",
        ));
    }
    if !first.endpoints().is_empty() && second.endpoints().is_empty() {
        return Err(SmokeError::Acceptance(
            "a camera found on the first scan must be reopenable on the second scan",
        ));
    }
    if options.hardware_context == HardwareContext::CameraPresent
        && (first.endpoints().is_empty() || second.endpoints().is_empty())
    {
        return Err(SmokeError::Acceptance(
            "camera-present context requires positive open/read/reopen evidence",
        ));
    }
    Ok(())
}

fn run_scan(
    service: &CameraService,
    policy: DeviceScanPolicy,
) -> Result<DeviceScanSnapshot, SmokeError> {
    let maximum_wait = policy
        .operation_deadline()
        .saturating_add(policy.shutdown_deadline())
        .saturating_add(Duration::from_secs(2));
    let started = service.start(policy)?;
    let started_at = Instant::now();
    loop {
        let snapshot = service.get(started.operation_id().as_str())?;
        if snapshot.status().is_terminal() {
            return Ok(snapshot);
        }
        if started_at.elapsed() > maximum_wait {
            return Err(SmokeError::Acceptance(
                "scan did not reach a terminal snapshot within the bounded wait",
            ));
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

fn write_evidence(path: &Path, evidence: &CameraSmokeEvidence) -> Result<(), SmokeError> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(SmokeError::CreateDirectory)?;
    }
    let bytes = serde_json::to_vec_pretty(evidence)?;
    fs::write(path, bytes).map_err(SmokeError::Write)
}

fn parse_options(arguments: impl IntoIterator<Item = String>) -> Result<Options, SmokeError> {
    let mut arguments = arguments.into_iter();
    let mut first_index = None;
    let mut last_index = None;
    let mut backends = None;
    let mut evidence_path = None;
    let mut hardware_context = None;
    let mut first_frame_deadline_ms = 5_000;
    let mut operation_deadline_ms = 90_000;
    let mut shutdown_deadline_ms = 3_000;
    let mut reopen_delay_ms = 500;

    while let Some(flag) = arguments.next() {
        let value = arguments
            .next()
            .ok_or_else(|| SmokeError::InvalidArgument(format!("missing value for {flag}")))?;
        match flag.as_str() {
            "--first-index" => first_index = Some(parse_number(&flag, &value)?),
            "--last-index" => last_index = Some(parse_number(&flag, &value)?),
            "--backends" => backends = Some(parse_backends(&value)?),
            "--evidence" => evidence_path = Some(PathBuf::from(value)),
            "--hardware-context" => {
                hardware_context = Some(match value.as_str() {
                    "camera-present" => HardwareContext::CameraPresent,
                    "camera-less" => HardwareContext::CameraLessHost,
                    _ => {
                        return Err(SmokeError::InvalidArgument(
                            "--hardware-context supports camera-present or camera-less".to_owned(),
                        ));
                    }
                });
            }
            "--first-frame-deadline-ms" => {
                first_frame_deadline_ms = parse_number(&flag, &value)?;
            }
            "--operation-deadline-ms" => {
                operation_deadline_ms = parse_number(&flag, &value)?;
            }
            "--shutdown-deadline-ms" => {
                shutdown_deadline_ms = parse_number(&flag, &value)?;
            }
            "--reopen-delay-ms" => reopen_delay_ms = parse_number(&flag, &value)?,
            _ => return Err(SmokeError::InvalidArgument(format!("unknown flag {flag}"))),
        }
    }

    Ok(Options {
        first_index: required(first_index, "--first-index")?,
        last_index: required(last_index, "--last-index")?,
        backends: required(backends, "--backends")?,
        evidence_path: required(evidence_path, "--evidence")?,
        hardware_context: required(hardware_context, "--hardware-context")?,
        first_frame_deadline_ms,
        operation_deadline_ms,
        shutdown_deadline_ms,
        reopen_delay_ms,
    })
}

fn required<T>(value: Option<T>, flag: &'static str) -> Result<T, SmokeError> {
    value.ok_or_else(|| SmokeError::InvalidArgument(format!("{flag} is required")))
}

fn parse_number<T>(flag: &str, value: &str) -> Result<T, SmokeError>
where
    T: std::str::FromStr,
{
    value.parse::<T>().map_err(|_| {
        SmokeError::InvalidArgument(format!("{flag} must contain a non-negative integer"))
    })
}

fn parse_backends(value: &str) -> Result<Vec<CaptureBackend>, SmokeError> {
    value
        .split(',')
        .map(|backend| match backend.trim() {
            "MSMF" => Ok(CaptureBackend::Msmf),
            "DSHOW" => Ok(CaptureBackend::Dshow),
            _ => Err(SmokeError::InvalidArgument(
                "--backends supports only comma-separated MSMF and DSHOW".to_owned(),
            )),
        })
        .collect()
}

impl From<xp_capture_lib::camera::CameraConfigError> for SmokeError {
    fn from(error: xp_capture_lib::camera::CameraConfigError) -> Self {
        Self::InvalidArgument(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_explicit_bounded_hardware_options() {
        let options = parse_options([
            "--first-index".to_owned(),
            "0".to_owned(),
            "--last-index".to_owned(),
            "2".to_owned(),
            "--backends".to_owned(),
            "MSMF,DSHOW".to_owned(),
            "--evidence".to_owned(),
            "evidence/camera-session-smoke.json".to_owned(),
            "--hardware-context".to_owned(),
            "camera-less".to_owned(),
        ])
        .expect("valid options parse");
        assert_eq!(options.first_index, 0);
        assert_eq!(options.last_index, 2);
        assert_eq!(options.hardware_context, HardwareContext::CameraLessHost);
        assert_eq!(
            options.backends,
            vec![CaptureBackend::Msmf, CaptureBackend::Dshow]
        );
    }

    #[test]
    fn rejects_missing_or_implicit_hardware_scope() {
        assert!(parse_options(Vec::<String>::new()).is_err());
        assert!(parse_backends("CAP_ANY").is_err());
    }
}
