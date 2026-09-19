pub mod camera;
mod cli;
pub mod self_check;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use camera::{
    CameraPublicErrorV1, CameraService, CameraServiceSnapshotV1, DeviceScanSnapshotV1,
    DeviceScanStartedV1, OpenCvCaptureAdapterFactory, ProfileProgressV1, ProfileReportV1,
    ProfileStartedV1, cancel_device_scan_for, cancel_profile_for, get_device_scan_for,
    get_profile_result_for, get_profile_status_for, start_device_scan_for, start_profile_for,
    stop_camera_for,
};
use cli::{StartupMode, parse_startup_mode};
use self_check::{
    AtomicFileSystemAdapter, OpenCvRuntimeAdapter, PublicError, RuntimeManifest, SelfCheckService,
    SelfCheckTransportReport, WindowsModuleAdapter,
};

const EMBEDDED_RUNTIME_MANIFEST: &str = include_str!("../../runtime/manifest.json");

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> i32 {
    match parse_startup_mode(std::env::args_os().skip(1)) {
        Ok(StartupMode::Gui) => run_gui(),
        Ok(StartupMode::SelfCheck { report_path }) => run_headless(&report_path),
        Err(message) => {
            eprintln!("{message}");
            20
        }
    }
}

fn run_gui() -> i32 {
    let camera_service = CameraService::new(Arc::new(OpenCvCaptureAdapterFactory::new()));
    let application = match tauri::Builder::default()
        .manage(camera_service)
        .invoke_handler(tauri::generate_handler![
            run_self_check,
            start_device_scan,
            get_device_scan,
            cancel_device_scan,
            start_profile,
            get_profile_status,
            get_profile_result,
            cancel_profile,
            stop_camera
        ])
        .build(tauri::generate_context!())
    {
        Ok(application) => application,
        Err(error) => {
            eprintln!("failed to run Tauri application: {error}");
            return 20;
        }
    };
    application.run(|application, event| {
        if matches!(event, tauri::RunEvent::ExitRequested { .. }) {
            let service = tauri::Manager::state::<CameraService>(application);
            if let Err(error) = service.stop() {
                eprintln!("camera_shutdown phase failed: {error}");
            }
        }
    });
    0
}

fn run_headless(report_path: &Path) -> i32 {
    let service = match build_self_check_service() {
        Ok(service) => service,
        Err(_) => return 20,
    };
    service.run_and_write(report_path)
}

#[tauri::command]
async fn run_self_check() -> Result<SelfCheckTransportReport, PublicError> {
    tauri::async_runtime::spawn_blocking(|| {
        let service = build_self_check_service()?;
        let outcome = service.run();
        Ok(SelfCheckTransportReport::from(&outcome.report))
    })
    .await
    .map_err(|_| PublicError::internal())?
}

#[tauri::command]
fn start_device_scan(
    service: tauri::State<'_, CameraService>,
    request_v1: serde_json::Value,
) -> Result<DeviceScanStartedV1, CameraPublicErrorV1> {
    start_device_scan_for(service.inner(), request_v1)
}

#[tauri::command]
fn get_device_scan(
    service: tauri::State<'_, CameraService>,
    request_v1: serde_json::Value,
) -> Result<DeviceScanSnapshotV1, CameraPublicErrorV1> {
    get_device_scan_for(service.inner(), request_v1)
}

#[tauri::command]
async fn cancel_device_scan(
    service: tauri::State<'_, CameraService>,
    request_v1: serde_json::Value,
) -> Result<DeviceScanSnapshotV1, CameraPublicErrorV1> {
    let service = service.inner().clone();
    tauri::async_runtime::spawn_blocking(move || cancel_device_scan_for(&service, request_v1))
        .await
        .map_err(|_| CameraPublicErrorV1::from(&camera::CameraServiceError::WorkerPanicked))?
}

#[tauri::command]
fn start_profile(
    service: tauri::State<'_, CameraService>,
    request_v1: serde_json::Value,
) -> Result<ProfileStartedV1, CameraPublicErrorV1> {
    start_profile_for(service.inner(), request_v1)
}

#[tauri::command]
fn get_profile_status(
    service: tauri::State<'_, CameraService>,
    request_v1: serde_json::Value,
) -> Result<ProfileProgressV1, CameraPublicErrorV1> {
    get_profile_status_for(service.inner(), request_v1)
}

#[tauri::command]
fn get_profile_result(
    service: tauri::State<'_, CameraService>,
    request_v1: serde_json::Value,
) -> Result<ProfileReportV1, CameraPublicErrorV1> {
    get_profile_result_for(service.inner(), request_v1)
}

#[tauri::command]
async fn cancel_profile(
    service: tauri::State<'_, CameraService>,
    request_v1: serde_json::Value,
) -> Result<ProfileProgressV1, CameraPublicErrorV1> {
    let service = service.inner().clone();
    tauri::async_runtime::spawn_blocking(move || cancel_profile_for(&service, request_v1))
        .await
        .map_err(|_| CameraPublicErrorV1::from(&camera::CameraServiceError::WorkerPanicked))?
}

#[tauri::command]
async fn stop_camera(
    service: tauri::State<'_, CameraService>,
) -> Result<CameraServiceSnapshotV1, CameraPublicErrorV1> {
    let service = service.inner().clone();
    tauri::async_runtime::spawn_blocking(move || stop_camera_for(&service))
        .await
        .map_err(|_| CameraPublicErrorV1::from(&camera::CameraServiceError::WorkerPanicked))?
}

fn build_self_check_service() -> Result<
    SelfCheckService<OpenCvRuntimeAdapter, WindowsModuleAdapter, AtomicFileSystemAdapter>,
    PublicError,
> {
    let manifest = match serde_json::from_str::<RuntimeManifest>(EMBEDDED_RUNTIME_MANIFEST) {
        Ok(manifest) => manifest,
        Err(_) => return Err(PublicError::internal()),
    };
    if manifest.validate().is_err() {
        return Err(PublicError::internal());
    }
    let install_root = match executable_directory() {
        Ok(path) => path,
        Err(_) => return Err(PublicError::internal()),
    };
    Ok(SelfCheckService::new(
        OpenCvRuntimeAdapter::new(),
        WindowsModuleAdapter::new(manifest, install_root, windows_system_directories()),
        AtomicFileSystemAdapter::new(),
    ))
}

fn executable_directory() -> Result<PathBuf, std::io::Error> {
    std::env::current_exe()?
        .parent()
        .map(Path::to_owned)
        .ok_or_else(|| std::io::Error::other("application executable has no parent directory"))
}

fn windows_system_directories() -> Vec<PathBuf> {
    let Some(windows_root) = std::env::var_os("WINDIR") else {
        return Vec::new();
    };
    ["System32", "SysWOW64"]
        .into_iter()
        .map(|directory| PathBuf::from(&windows_root).join(directory))
        .collect()
}
