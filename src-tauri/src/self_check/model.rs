use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const SELF_CHECK_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Passed,
    Failed,
    Skipped,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckCode {
    #[serde(rename = "opencv_load")]
    OpenCvLoad,
    ImageCodec,
    WriterOpen,
    WriterBackend,
    WriterRoundtrip,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CheckResult {
    pub check: CheckCode,
    pub status: CheckStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_message: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OpenCvSummary {
    pub version: String,
    pub build_information_sha256: String,
    pub available_backends: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelfCheckOutcome {
    pub report: SelfCheckReport,
    pub exit_code: i32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LoadedModule {
    pub name: String,
    pub canonical_path: PathBuf,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SelfCheckReport {
    pub schema_version: u32,
    pub checks: Vec<CheckResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opencv: Option<OpenCvSummary>,
    pub loaded_modules: Vec<LoadedModule>,
}

impl SelfCheckReport {
    pub fn empty() -> Self {
        Self {
            schema_version: SELF_CHECK_SCHEMA_VERSION,
            checks: Vec::new(),
            opencv: None,
            loaded_modules: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SELF_CHECK_SCHEMA_VERSION, SelfCheckReport};

    #[test]
    fn serializes_schema_version() {
        let value = serde_json::to_value(SelfCheckReport::empty()).expect("report serializes");

        assert_eq!(value["schema_version"], SELF_CHECK_SCHEMA_VERSION);
    }
}
