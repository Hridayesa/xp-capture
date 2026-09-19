use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use serde::Serialize;
use thiserror::Error;
use xp_capture_lib::camera::{
    CameraService, CameraServiceError, CaptureBackend, DeviceScanPolicy, DeviceScanStatus,
    OpenCvCaptureAdapterFactory, ProfileReportV1,
    profiling::{ModeCandidatePolicy, ModeCandidatePolicyV1, ProfileOperationStatus},
};

const EVIDENCE_SCHEMA_VERSION: u32 = 1;
const POLL_INTERVAL: Duration = Duration::from_millis(25);
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Error)]
enum SmokeError {
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    #[error("camera service failed")]
    Camera(#[from] CameraServiceError),
    #[error("config read failed")]
    ConfigRead(#[source] std::io::Error),
    #[error("config JSON is invalid")]
    ConfigJson(#[source] serde_json::Error),
    #[error("evidence serialization failed")]
    Serialize(#[from] serde_json::Error),
    #[error("evidence I/O failed")]
    EvidenceIo(#[source] std::io::Error),
    #[error("profile smoke did not satisfy its acceptance gate: {0}")]
    Acceptance(&'static str),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Options {
    backend: CaptureBackend,
    numeric_index: u32,
    config_path: PathBuf,
    evidence_path: PathBuf,
}

#[derive(Serialize)]
struct ProfileSmokeEvidence {
    schema_version: u32,
    evidence_kind: &'static str,
    selected_backend: String,
    numeric_index: u32,
    profile: ProfileReportV1,
    release_reopen: ReleaseReopenEvidence,
}

#[derive(Serialize)]
struct ReleaseReopenEvidence {
    profile_terminal: bool,
    handle_released_before_terminal: bool,
    rescan_completed: bool,
    endpoint_reopened: bool,
}

fn main() -> std::process::ExitCode {
    match run(std::env::args().skip(1)) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("camera_mode_profile_smoke failed: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run(arguments: impl IntoIterator<Item = String>) -> Result<(), SmokeError> {
    let options = parse_options(arguments)?;
    let config_json = fs::read_to_string(&options.config_path).map_err(SmokeError::ConfigRead)?;
    let config_wire: ModeCandidatePolicyV1 =
        serde_json::from_str(&config_json).map_err(SmokeError::ConfigJson)?;
    let validated = ModeCandidatePolicy::validate(config_wire.clone())?;
    let scan_policy = DeviceScanPolicy::new(
        options.numeric_index,
        options.numeric_index,
        vec![options.backend],
        duration_ms(validated.first_frame_deadline()),
        duration_ms(validated.operation_deadline()),
        duration_ms(validated.shutdown_deadline()),
        duration_ms(validated.reopen_delay()),
    )?;
    let service = CameraService::new(Arc::new(OpenCvCaptureAdapterFactory::new()));
    let scan = run_scan(&service, scan_policy.clone())?;
    let endpoint = scan
        .endpoints()
        .iter()
        .find(|endpoint| {
            endpoint.target().backend() == options.backend
                && endpoint.target().numeric_index() == options.numeric_index
        })
        .ok_or(SmokeError::Acceptance(
            "explicit backend/index was not available in the bounded scan",
        ))?;
    let started = service.start_profile(endpoint.key().as_str(), config_wire)?;
    let profile_id = started.profile_id.as_str().to_owned();
    let profile_status = wait_profile(&service, &profile_id, validated.operation_deadline())?;
    let report = service.get_profile_result(&profile_id)?;
    let report_v1 = ProfileReportV1::from(&report);
    let rescan = run_scan(&service, scan_policy)?;
    let endpoint_reopened = rescan.endpoints().iter().any(|endpoint| {
        endpoint.target().backend() == options.backend
            && endpoint.target().numeric_index() == options.numeric_index
    });
    let stopped = service.stop()?;
    let evidence = ProfileSmokeEvidence {
        schema_version: EVIDENCE_SCHEMA_VERSION,
        evidence_kind: "opencv-camera-mode-profile-smoke",
        selected_backend: options.backend.name().to_owned(),
        numeric_index: options.numeric_index,
        profile: report_v1,
        release_reopen: ReleaseReopenEvidence {
            profile_terminal: profile_status.status.is_terminal(),
            handle_released_before_terminal: matches!(
                profile_status.status,
                ProfileOperationStatus::Completed
                    | ProfileOperationStatus::Cancelled
                    | ProfileOperationStatus::Failed
            ),
            rescan_completed: rescan.status() == DeviceScanStatus::Completed,
            endpoint_reopened,
        },
    };
    write_compact_json_atomically(&options.evidence_path, &evidence)?;

    if profile_status.status != ProfileOperationStatus::Completed {
        return Err(SmokeError::Acceptance("profile must complete"));
    }
    if report.results.is_empty() {
        return Err(SmokeError::Acceptance(
            "profile must contain at least one measured candidate outcome",
        ));
    }
    if rescan.status() != DeviceScanStatus::Completed || !endpoint_reopened {
        return Err(SmokeError::Acceptance(
            "profile cleanup must permit the explicit endpoint to reopen",
        ));
    }
    if stopped.service_state() != xp_capture_lib::camera::CameraServiceState::Idle {
        return Err(SmokeError::Acceptance("stop must leave the service idle"));
    }
    Ok(())
}

fn run_scan(
    service: &CameraService,
    policy: DeviceScanPolicy,
) -> Result<xp_capture_lib::camera::DeviceScanSnapshot, SmokeError> {
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
            return Err(SmokeError::Acceptance("bounded scan wait expired"));
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

fn wait_profile(
    service: &CameraService,
    profile_id: &str,
    operation_deadline: Duration,
) -> Result<xp_capture_lib::camera::profiling::ProfileSnapshot, SmokeError> {
    let maximum_wait = operation_deadline.saturating_add(Duration::from_secs(5));
    let started_at = Instant::now();
    loop {
        let snapshot = service.get_profile_status(profile_id)?;
        if snapshot.status.is_terminal() {
            return Ok(snapshot);
        }
        if started_at.elapsed() > maximum_wait {
            return Err(SmokeError::Acceptance("bounded profile wait expired"));
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

fn write_compact_json_atomically(
    path: &Path,
    evidence: &ProfileSmokeEvidence,
) -> Result<(), SmokeError> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(SmokeError::EvidenceIo)?;
    }
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("camera-mode-profile-smoke.json");
    let temporary = path.with_file_name(format!(".{file_name}.{}.{}.tmp", process::id(), sequence));
    let mut cleanup = TemporaryEvidence::new(temporary.clone());
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(SmokeError::EvidenceIo)?;
    file.write_all(&serde_json::to_vec(evidence)?)
        .and_then(|()| file.sync_all())
        .map_err(SmokeError::EvidenceIo)?;
    drop(file);
    replace_file(&temporary, path).map_err(SmokeError::EvidenceIo)?;
    cleanup.disarm();
    Ok(())
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let succeeded = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if succeeded == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::rename(source, destination)
}

struct TemporaryEvidence {
    path: Option<PathBuf>,
}

impl TemporaryEvidence {
    fn new(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    fn disarm(&mut self) {
        self.path = None;
    }
}

impl Drop for TemporaryEvidence {
    fn drop(&mut self) {
        if let Some(path) = &self.path {
            let _ = fs::remove_file(path);
        }
    }
}

fn parse_options(arguments: impl IntoIterator<Item = String>) -> Result<Options, SmokeError> {
    let mut arguments = arguments.into_iter();
    let mut backend = None;
    let mut numeric_index = None;
    let mut config_path = None;
    let mut evidence_path = None;
    while let Some(flag) = arguments.next() {
        let value = arguments
            .next()
            .ok_or_else(|| SmokeError::InvalidArgument(format!("missing value for {flag}")))?;
        match flag.as_str() {
            "--backend" => {
                backend = Some(match value.as_str() {
                    "MSMF" => CaptureBackend::Msmf,
                    "DSHOW" => CaptureBackend::Dshow,
                    _ => {
                        return Err(SmokeError::InvalidArgument(
                            "--backend supports MSMF or DSHOW".to_owned(),
                        ));
                    }
                });
            }
            "--index" => {
                numeric_index = Some(value.parse().map_err(|_| {
                    SmokeError::InvalidArgument("--index must be a non-negative integer".to_owned())
                })?);
            }
            "--config" => config_path = Some(PathBuf::from(value)),
            "--evidence" => evidence_path = Some(PathBuf::from(value)),
            _ => return Err(SmokeError::InvalidArgument(format!("unknown flag {flag}"))),
        }
    }
    Ok(Options {
        backend: required(backend, "--backend")?,
        numeric_index: required(numeric_index, "--index")?,
        config_path: required(config_path, "--config")?,
        evidence_path: required(evidence_path, "--evidence")?,
    })
}

fn required<T>(value: Option<T>, flag: &'static str) -> Result<T, SmokeError> {
    value.ok_or_else(|| SmokeError::InvalidArgument(format!("{flag} is required")))
}

fn duration_ms(value: Duration) -> u64 {
    u64::try_from(value.as_millis()).unwrap_or(u64::MAX)
}

impl From<xp_capture_lib::camera::CameraConfigError> for SmokeError {
    fn from(error: xp_capture_lib::camera::CameraConfigError) -> Self {
        Self::InvalidArgument(error.to_string())
    }
}

impl From<xp_capture_lib::camera::profiling::ProfileConfigError> for SmokeError {
    fn from(error: xp_capture_lib::camera::profiling::ProfileConfigError) -> Self {
        Self::InvalidArgument(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_explicit_bounded_profile_scope() {
        let options = parse_options([
            "--backend".to_owned(),
            "DSHOW".to_owned(),
            "--index".to_owned(),
            "0".to_owned(),
            "--config".to_owned(),
            "config/mode-candidates.json".to_owned(),
            "--evidence".to_owned(),
            "evidence/camera-mode-profile-smoke.json".to_owned(),
        ])
        .expect("options");
        assert_eq!(options.backend, CaptureBackend::Dshow);
        assert_eq!(options.numeric_index, 0);
    }

    #[test]
    fn rejects_missing_scope_cap_any_and_invalid_index() {
        assert!(parse_options(Vec::<String>::new()).is_err());
        assert!(
            parse_options([
                "--backend".to_owned(),
                "CAP_ANY".to_owned(),
                "--index".to_owned(),
                "0".to_owned(),
                "--config".to_owned(),
                "config.json".to_owned(),
                "--evidence".to_owned(),
                "evidence.json".to_owned(),
            ])
            .is_err()
        );
    }
}
