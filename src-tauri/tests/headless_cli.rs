#![cfg(windows)]

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

use xp_capture_lib::self_check::{CheckStatus, RuntimeManifest, SelfCheckReport};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TestInstallation {
    root: PathBuf,
    executable: PathBuf,
    report: PathBuf,
}

impl TestInstallation {
    fn new(stage_runtime: bool) -> Self {
        let root = std::env::temp_dir().join(format!(
            "xp-capture-headless-test-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).expect("test installation directory is created");
        let executable = root.join("xp-capture.exe");
        fs::copy(env!("CARGO_BIN_EXE_xp-capture"), &executable)
            .expect("application executable is staged");
        let report = root.join("evidence").join("self-check.json");

        if stage_runtime {
            let repository_root = Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .expect("crate has repository parent");
            let manifest_path = repository_root.join("runtime").join("manifest.json");
            let manifest: RuntimeManifest = serde_json::from_slice(
                &fs::read(&manifest_path).expect("runtime manifest can be read"),
            )
            .expect("runtime manifest can be parsed");
            for entry in &manifest.files {
                let source = repository_root.join(Path::new(&entry.source));
                let destination = root.join(Path::new(&entry.bundle_destination));
                fs::create_dir_all(destination.parent().expect("runtime file has parent"))
                    .expect("runtime destination is created");
                fs::copy(source, destination).expect("manifest runtime file is staged");
            }
            let installed_manifest = root.join("runtime").join("manifest.json");
            fs::create_dir_all(installed_manifest.parent().expect("manifest has parent"))
                .expect("installed manifest directory is created");
            fs::copy(manifest_path, installed_manifest).expect("runtime manifest is staged");
        }

        Self {
            root,
            executable,
            report,
        }
    }

    fn run(&self, extra_path: Option<&Path>) -> Output {
        let windows_root = PathBuf::from(std::env::var_os("WINDIR").expect("WINDIR is set"));
        let system_path = format!(
            "{};{}",
            windows_root.join("System32").display(),
            windows_root.display()
        );
        let path = extra_path
            .map(|path| format!("{};{system_path}", path.display()))
            .unwrap_or(system_path);
        let mut command = Command::new(&self.executable);
        command
            .current_dir(&self.root)
            .args(["--self-check", "--json"])
            .arg(&self.report)
            .env("PATH", path);
        for (name, _) in std::env::vars_os() {
            let normalized = name.to_string_lossy().to_ascii_uppercase();
            if normalized.starts_with("OPENCV_") || normalized.starts_with("VCPKG_") {
                command.env_remove(name);
            }
        }
        command.output().expect("headless application starts")
    }
}

impl Drop for TestInstallation {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn staged_headless_self_check_exits_without_gui_and_writes_report() {
    let installation = TestInstallation::new(true);
    let output = installation.run(None);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: SelfCheckReport = serde_json::from_slice(
        &fs::read(&installation.report).expect("headless report was written"),
    )
    .expect("headless report is valid");
    assert!(
        report
            .checks
            .iter()
            .all(|check| check.status == CheckStatus::Passed)
    );
    assert!(!report.loaded_modules.is_empty());
}

#[test]
fn build_tree_module_origin_returns_10_and_preserves_report() {
    let installation = TestInstallation::new(false);
    let repository_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate has repository parent");
    let build_tree_bin = repository_root
        .join(".vcpkg_installed")
        .join("x64-windows")
        .join("bin");
    let output = installation.run(Some(&build_tree_bin));

    assert_eq!(
        output.status.code(),
        Some(10),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: SelfCheckReport = serde_json::from_slice(
        &fs::read(&installation.report).expect("failure report was preserved"),
    )
    .expect("failure report is valid");
    assert!(report.loaded_modules.is_empty());
}
