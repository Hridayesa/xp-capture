use serde_json::Value;

const CAPABILITY: &str = include_str!("../capabilities/diagnostics.json");
const CONFIG: &str = include_str!("../tauri.conf.json");

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
