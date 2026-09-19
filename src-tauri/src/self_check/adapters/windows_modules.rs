use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

use crate::self_check::{
    AdapterError, LoadedModule, ModuleAdapter, RuntimeManifest, RuntimeSupplyError,
};

pub struct WindowsModuleAdapter {
    manifest: RuntimeManifest,
    install_root: PathBuf,
    system_directories: Vec<PathBuf>,
}

impl WindowsModuleAdapter {
    pub fn from_manifest_file(
        manifest_path: &Path,
        install_root: PathBuf,
    ) -> Result<Self, RuntimeSupplyError> {
        let manifest_bytes =
            fs::read(manifest_path).map_err(|source| RuntimeSupplyError::ManifestRead {
                path: manifest_path.to_owned(),
                source,
            })?;
        let manifest =
            serde_json::from_slice::<RuntimeManifest>(&manifest_bytes).map_err(|source| {
                RuntimeSupplyError::ManifestParse {
                    path: manifest_path.to_owned(),
                    source,
                }
            })?;
        manifest.validate()?;

        Ok(Self::new(
            manifest,
            install_root,
            canonical_windows_system_directories(),
        ))
    }

    pub fn new(
        manifest: RuntimeManifest,
        install_root: PathBuf,
        system_directories: Vec<PathBuf>,
    ) -> Self {
        Self {
            manifest,
            install_root,
            system_directories,
        }
    }

    fn inspect_paths(&self, paths: Vec<PathBuf>) -> Result<Vec<LoadedModule>, RuntimeSupplyError> {
        validate_module_paths(
            &self.manifest,
            &self.install_root,
            &self.system_directories,
            paths,
        )
    }
}

impl ModuleAdapter for WindowsModuleAdapter {
    fn loaded_modules(&self) -> Result<Vec<LoadedModule>, AdapterError> {
        let paths = enumerate_current_process_modules().map_err(|source| {
            AdapterError::with_source("failed to enumerate current process modules", source)
        })?;
        self.inspect_paths(paths).map_err(|source| {
            AdapterError::with_source("runtime module provenance validation failed", source)
        })
    }
}

fn validate_module_paths(
    manifest: &RuntimeManifest,
    install_root: &Path,
    system_directories: &[PathBuf],
    paths: Vec<PathBuf>,
) -> Result<Vec<LoadedModule>, RuntimeSupplyError> {
    manifest.validate()?;
    let canonical_install_root = canonicalize(install_root)?;
    let canonical_system_directories = system_directories
        .iter()
        .filter_map(|path| fs::canonicalize(path).ok())
        .collect::<Vec<_>>();
    let allowlist = manifest
        .files
        .iter()
        .map(|file| (file.name.to_ascii_lowercase(), file))
        .collect::<HashMap<_, _>>();
    let mut observed = HashSet::new();
    let mut loaded = Vec::new();

    for path in paths {
        let name = match path.file_name().and_then(|name| name.to_str()) {
            Some(name) => name,
            None => continue,
        };
        let normalized_name = name.to_ascii_lowercase();
        let Some(expected) = allowlist.get(&normalized_name) else {
            if is_runtime_candidate(&normalized_name)
                && !is_in_any_directory(&path, &canonical_system_directories)
            {
                return Err(RuntimeSupplyError::ModuleNotAllowed {
                    module: name.to_owned(),
                });
            }
            continue;
        };

        let canonical_path = canonicalize(&path)?;
        let expected_path =
            canonicalize(&canonical_install_root.join(Path::new(&expected.bundle_destination)))?;
        if normalize_path(&canonical_path) != normalize_path(&expected_path) {
            return Err(RuntimeSupplyError::InvalidOrigin {
                module: name.to_owned(),
            });
        }

        let sha256 = sha256_file(&canonical_path)?;
        if !sha256.eq_ignore_ascii_case(&expected.sha256) {
            return Err(RuntimeSupplyError::HashMismatch {
                module: name.to_owned(),
            });
        }
        observed.insert(normalized_name);
        loaded.push(LoadedModule {
            name: expected.name.clone(),
            canonical_path,
            sha256,
        });
    }

    for name in allowlist.keys() {
        if !observed.contains(name) {
            return Err(RuntimeSupplyError::ModuleNotAllowed {
                module: name.clone(),
            });
        }
    }

    loaded.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(loaded)
}

fn canonicalize(path: &Path) -> Result<PathBuf, RuntimeSupplyError> {
    fs::canonicalize(path).map_err(|source| RuntimeSupplyError::ManifestRead {
        path: path.to_owned(),
        source,
    })
}

fn sha256_file(path: &Path) -> Result<String, RuntimeSupplyError> {
    let bytes = fs::read(path).map_err(|source| RuntimeSupplyError::ManifestRead {
        path: path.to_owned(),
        source,
    })?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy()
        .trim_start_matches(r"\\?\")
        .replace('/', "\\")
        .to_ascii_lowercase()
}

fn is_in_any_directory(path: &Path, directories: &[PathBuf]) -> bool {
    let Ok(canonical_path) = fs::canonicalize(path) else {
        return false;
    };
    let normalized_path = normalize_path(&canonical_path);
    directories.iter().any(|directory| {
        let normalized_directory = normalize_path(directory);
        normalized_path == normalized_directory
            || normalized_path.starts_with(&(normalized_directory + "\\"))
    })
}

fn is_runtime_candidate(name: &str) -> bool {
    name.starts_with("opencv_")
        || name.starts_with("avcodec-")
        || name.starts_with("avformat-")
        || name.starts_with("avutil-")
        || name.starts_with("swresample-")
        || name.starts_with("swscale-")
        || name.starts_with("jpeg")
        || name == "z.dll"
        || matches!(
            name,
            "concrt140.dll"
                | "msvcp140.dll"
                | "msvcp140_1.dll"
                | "msvcp140_2.dll"
                | "msvcp140_atomic_wait.dll"
                | "msvcp140_codecvt_ids.dll"
                | "vccorlib140.dll"
                | "vcruntime140.dll"
                | "vcruntime140_1.dll"
                | "vcruntime140_threads.dll"
        )
}

fn canonical_windows_system_directories() -> Vec<PathBuf> {
    let Some(windows_root) = std::env::var_os("WINDIR") else {
        return Vec::new();
    };
    ["System32", "SysWOW64"]
        .into_iter()
        .filter_map(|directory| fs::canonicalize(PathBuf::from(&windows_root).join(directory)).ok())
        .collect()
}

#[cfg(windows)]
fn enumerate_current_process_modules() -> Result<Vec<PathBuf>, std::io::Error> {
    use std::{mem::size_of, ptr};
    use windows_sys::Win32::{
        Foundation::HMODULE,
        System::{
            ProcessStatus::{K32EnumProcessModules, K32GetModuleFileNameExW},
            Threading::GetCurrentProcess,
        },
    };

    let process = unsafe { GetCurrentProcess() };
    let mut modules: Vec<HMODULE> = vec![ptr::null_mut(); 128];
    loop {
        let mut bytes_needed = 0_u32;
        let buffer_bytes = u32::try_from(modules.len() * size_of::<HMODULE>())
            .map_err(|_| std::io::Error::other("module buffer exceeds Win32 limits"))?;
        let succeeded = unsafe {
            K32EnumProcessModules(
                process,
                modules.as_mut_ptr(),
                buffer_bytes,
                &mut bytes_needed,
            )
        };
        if succeeded == 0 {
            return Err(std::io::Error::last_os_error());
        }
        if bytes_needed <= buffer_bytes {
            modules.truncate(bytes_needed as usize / size_of::<HMODULE>());
            break;
        }
        modules.resize(
            bytes_needed as usize / size_of::<HMODULE>() + 16,
            ptr::null_mut(),
        );
    }

    modules
        .into_iter()
        .map(|module| {
            let mut buffer = vec![0_u16; 32_768];
            let length = unsafe {
                K32GetModuleFileNameExW(process, module, buffer.as_mut_ptr(), buffer.len() as u32)
            };
            if length == 0 {
                return Err(std::io::Error::last_os_error());
            }
            buffer.truncate(length as usize);
            Ok(PathBuf::from(String::from_utf16_lossy(&buffer)))
        })
        .collect()
}

#[cfg(not(windows))]
fn enumerate_current_process_modules() -> Result<Vec<PathBuf>, std::io::Error> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "process module enumeration is supported only on Windows",
    ))
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    use sha2::{Digest, Sha256};

    use super::validate_module_paths;
    use crate::self_check::{
        PeArchitecture, RUNTIME_MANIFEST_SCHEMA_VERSION, RuntimeFilePurpose, RuntimeManifest,
        RuntimeManifestFile,
    };

    static SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct Fixture {
        root: PathBuf,
        install_root: PathBuf,
        system_root: PathBuf,
        manifest: RuntimeManifest,
        installed_module: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "xp-capture-module-test-{}-{}",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            let install_root = root.join("installed");
            let system_root = root.join("Windows").join("System32");
            fs::create_dir_all(&install_root).expect("install fixture directory is created");
            fs::create_dir_all(&system_root).expect("system fixture directory is created");
            let installed_module = install_root.join("opencv_core4.dll");
            fs::write(&installed_module, b"expected module")
                .expect("installed module fixture is written");
            let hash = format!("{:x}", Sha256::digest(b"expected module"));
            let manifest = RuntimeManifest {
                schema_version: RUNTIME_MANIFEST_SCHEMA_VERSION,
                generated_at_utc: "2026-09-19T00:00:00Z".to_owned(),
                vcpkg_revision: "9e593bb18ea69cc5095e012465dcd675a822ed0d".to_owned(),
                triplet: "x64-windows".to_owned(),
                files: vec![RuntimeManifestFile {
                    name: "opencv_core4.dll".to_owned(),
                    sha256: hash,
                    architecture: PeArchitecture::X64,
                    source: ".vcpkg_installed/x64-windows/bin/opencv_core4.dll".to_owned(),
                    bundle_destination: "opencv_core4.dll".to_owned(),
                    purpose: RuntimeFilePurpose::OpenCv,
                    license_notice_path: ".vcpkg_installed/x64-windows/share/opencv4/copyright"
                        .to_owned(),
                }],
            };
            Self {
                root,
                install_root,
                system_root,
                manifest,
                installed_module,
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn validate(fixture: &Fixture, paths: Vec<PathBuf>) -> Result<(), String> {
        validate_module_paths(
            &fixture.manifest,
            &fixture.install_root,
            std::slice::from_ref(&fixture.system_root),
            paths,
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
    }

    #[test]
    fn accepts_manifest_module_from_installed_origin_and_system_module() {
        let fixture = Fixture::new();
        let system_module = fixture.system_root.join("kernel32.dll");
        fs::write(&system_module, b"system").expect("system module fixture is written");

        validate(
            &fixture,
            vec![fixture.installed_module.clone(), system_module],
        )
        .expect("installed and system origins pass");
    }

    #[test]
    fn rejects_hash_mismatch() {
        let fixture = Fixture::new();
        fs::write(&fixture.installed_module, b"tampered").expect("fixture is changed");

        let error = validate(&fixture, vec![fixture.installed_module.clone()])
            .expect_err("hash mismatch must fail");
        assert!(error.contains("hash mismatch"));
    }

    #[test]
    fn rejects_manifest_module_loaded_from_build_tree() {
        let fixture = Fixture::new();
        let build_module = fixture
            .root
            .join(".vcpkg_installed")
            .join("x64-windows")
            .join("bin")
            .join("opencv_core4.dll");
        fs::create_dir_all(build_module.parent().unwrap_or(Path::new(".")))
            .expect("build fixture directory is created");
        fs::write(&build_module, b"expected module").expect("build module fixture is written");

        let error =
            validate(&fixture, vec![build_module]).expect_err("build-tree origin must fail");
        assert!(error.contains("origin is not allowed"));
    }
}
