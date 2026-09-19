use serde_json::Value;

const CAPABILITY: &str = include_str!("../capabilities/diagnostics.json");
const CONFIG: &str = include_str!("../tauri.conf.json");
const APPLICATION_SOURCE: &str = include_str!("../src/lib.rs");

#[test]
fn diagnostic_capability_has_no_unplanned_permissions() {
    let capability: Value = serde_json::from_str(CAPABILITY).expect("capability is valid JSON");
    let permissions = capability["permissions"]
        .as_array()
        .expect("permissions is an array");

    assert!(permissions.is_empty());
    assert_eq!(capability["windows"], serde_json::json!(["main"]));
}

#[test]
fn csp_allows_only_local_ui_and_tauri_ipc() {
    let config: Value = serde_json::from_str(CONFIG).expect("Tauri config is valid JSON");
    let csp = config["app"]["security"]["csp"]
        .as_str()
        .expect("CSP is a string");

    assert!(csp.contains("default-src 'self'"));
    assert!(csp.contains("connect-src ipc: http://ipc.localhost"));
    assert!(!csp.contains("https:"));
    assert!(!csp.contains("*"));
}

#[test]
fn windows_bundle_is_current_user_offline_nsis_with_manifest_staging() {
    let config: Value = serde_json::from_str(CONFIG).expect("Tauri config is valid JSON");

    assert_eq!(config["bundle"]["targets"], serde_json::json!(["nsis"]));
    assert_eq!(
        config["bundle"]["windows"]["webviewInstallMode"]["type"],
        "offlineInstaller"
    );
    assert_eq!(
        config["bundle"]["windows"]["nsis"]["installMode"],
        "currentUser"
    );
    assert_eq!(config["bundle"]["resources"]["../runtime/staging/"], "");
    assert_eq!(
        config["bundle"]["resources"]["../runtime/manifest.json"],
        "runtime/manifest.json"
    );
    assert!(config["build"].get("staticVCRuntime").is_none());
    assert!(config["bundle"]["windows"].get("bundleVCRuntime").is_none());
}

#[test]
fn camera_commands_use_one_managed_service_and_shutdown_uses_stop_path() {
    assert!(APPLICATION_SOURCE.contains(".manage(camera_service)"));
    for command in [
        "start_device_scan",
        "get_device_scan",
        "cancel_device_scan",
        "start_profile",
        "get_profile_status",
        "get_profile_result",
        "cancel_profile",
        "stop_camera",
    ] {
        assert!(APPLICATION_SOURCE.contains(command));
    }
    assert!(APPLICATION_SOURCE.contains("tauri::RunEvent::ExitRequested"));
    assert!(APPLICATION_SOURCE.contains("service.stop()"));
    assert!(!APPLICATION_SOURCE.contains("VideoCapture"));
}
