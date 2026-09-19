mod cli;
pub mod self_check;

use std::path::{Path, PathBuf};

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
    match tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![run_self_check])
        .run(tauri::generate_context!())
    {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("failed to run Tauri application: {error}");
            20
        }
    }
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
