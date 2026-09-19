use serde::{Deserialize, Serialize};

use super::RuntimeSupplyError;

pub const RUNTIME_MANIFEST_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeManifest {
    pub schema_version: u32,
    pub generated_at_utc: String,
    pub vcpkg_revision: String,
    pub triplet: String,
    pub files: Vec<RuntimeManifestFile>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeManifestFile {
    pub name: String,
    pub sha256: String,
    pub architecture: PeArchitecture,
    pub source: String,
    pub bundle_destination: String,
    pub purpose: RuntimeFilePurpose,
    pub license_notice_path: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum PeArchitecture {
    #[serde(rename = "x64")]
    X64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeFilePurpose {
    #[serde(rename = "opencv")]
    OpenCv,
    Codec,
    Transitive,
}

impl RuntimeManifest {
    pub fn validate(&self) -> Result<(), RuntimeSupplyError> {
        if self.schema_version != RUNTIME_MANIFEST_SCHEMA_VERSION {
            return Err(RuntimeSupplyError::InvalidManifest {
                reason: "unsupported schema version".to_owned(),
            });
        }
        if self.triplet != "x64-windows" {
            return Err(RuntimeSupplyError::InvalidManifest {
                reason: "unsupported triplet".to_owned(),
            });
        }
        if self.vcpkg_revision.len() != 40
            || !self
                .vcpkg_revision
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(RuntimeSupplyError::InvalidManifest {
                reason: "invalid vcpkg revision".to_owned(),
            });
        }
        if self.files.is_empty() {
            return Err(RuntimeSupplyError::InvalidManifest {
                reason: "runtime file allowlist is empty".to_owned(),
            });
        }

        for file in &self.files {
            if !is_explicit_dll_name(&file.name)
                || !is_sha256(&file.sha256)
                || !is_safe_relative_path(&file.source)
                || !is_safe_relative_path(&file.bundle_destination)
                || !is_safe_relative_path(&file.license_notice_path)
            {
                return Err(RuntimeSupplyError::InvalidManifest {
                    reason: format!("invalid runtime file metadata: {}", file.name),
                });
            }
        }

        Ok(())
    }
}

fn is_explicit_dll_name(value: &str) -> bool {
    value.to_ascii_lowercase().ends_with(".dll")
        && !value.contains(['*', '?', '/', '\\'])
        && !value.is_empty()
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn is_safe_relative_path(value: &str) -> bool {
    !value.is_empty()
        && !value.contains(['*', '?', '\\'])
        && !value.starts_with('/')
        && !value.split('/').any(|segment| segment == "..")
}

#[cfg(test)]
mod tests {
    use super::{RUNTIME_MANIFEST_SCHEMA_VERSION, RuntimeManifest};

    #[test]
    fn rust_model_accepts_schema_fixture() {
        let manifest: RuntimeManifest = serde_json::from_str(include_str!(
            "../../../tools/tests/fixtures/runtime-manifest-valid.json"
        ))
        .expect("valid fixture deserializes");

        assert_eq!(manifest.schema_version, RUNTIME_MANIFEST_SCHEMA_VERSION);
        manifest
            .validate()
            .expect("valid fixture passes model checks");
    }

    #[test]
    fn rust_model_rejects_wildcard_fixture() {
        let manifest: RuntimeManifest = serde_json::from_str(include_str!(
            "../../../tools/tests/fixtures/runtime-manifest-wildcard.json"
        ))
        .expect("wildcard fixture has valid JSON shape");

        assert!(manifest.validate().is_err());
    }
}
