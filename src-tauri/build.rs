use std::{env, fs, path::Path, time::SystemTime};

fn main() {
    println!("cargo:rerun-if-env-changed=XP_CAPTURE_BUILD_ENV_EVIDENCE");
    println!("cargo:rerun-if-env-changed=STATIC_VCRUNTIME");
    if let Some(evidence_path) = env::var_os("XP_CAPTURE_BUILD_ENV_EVIDENCE") {
        let static_vcruntime = env::var("STATIC_VCRUNTIME").unwrap_or_default();
        if static_vcruntime != "true" {
            panic!("Tauri build must provide effective STATIC_VCRUNTIME=true");
        }
        let target = env::var("TARGET").unwrap_or_else(|_| "unknown".to_owned());
        let generated_at_unix_seconds = SystemTime::UNIX_EPOCH
            .elapsed()
            .map(|duration| duration.as_secs())
            .unwrap_or_default();
        let evidence = format!(
            concat!(
                "{{\n",
                "  \"schemaVersion\": 1,\n",
                "  \"generatedAtUnixSeconds\": {},\n",
                "  \"target\": \"{}\",\n",
                "  \"tauriCrate\": \"2.11.5\",\n",
                "  \"tauriCli\": \"2.11.4\",\n",
                "  \"staticVcruntime\": true\n",
                "}}\n"
            ),
            generated_at_unix_seconds, target
        );
        write_evidence(Path::new(&evidence_path), evidence.as_bytes());
    }
    tauri_build::build()
}

fn write_evidence(path: &Path, bytes: &[u8]) {
    let Some(parent) = path.parent() else {
        panic!("STATIC_VCRUNTIME evidence path must have a parent directory");
    };
    if let Err(error) = fs::create_dir_all(parent) {
        panic!("failed to create STATIC_VCRUNTIME evidence directory: {error}");
    }
    let temporary = path.with_extension("json.tmp");
    if let Err(error) = fs::write(&temporary, bytes) {
        panic!("failed to write STATIC_VCRUNTIME evidence: {error}");
    }
    if path.exists()
        && let Err(error) = fs::remove_file(path)
    {
        panic!("failed to replace STATIC_VCRUNTIME evidence: {error}");
    }
    if let Err(error) = fs::rename(&temporary, path) {
        panic!("failed to commit STATIC_VCRUNTIME evidence: {error}");
    }
}
