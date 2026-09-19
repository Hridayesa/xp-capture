use serde::{Deserialize, Serialize};

use super::{CheckResult, SelfCheckError, SelfCheckReport};

pub const TRANSPORT_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SelfCheckTransportReport {
    pub schema_version: u32,
    pub checks: Vec<CheckResult>,
    pub opencv_version: Option<String>,
}

impl From<&SelfCheckReport> for SelfCheckTransportReport {
    fn from(report: &SelfCheckReport) -> Self {
        Self {
            schema_version: TRANSPORT_SCHEMA_VERSION,
            checks: report.checks.clone(),
            opencv_version: report
                .opencv
                .as_ref()
                .map(|summary| summary.version.clone()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PublicError {
    pub schema_version: u32,
    pub code: String,
    pub message: String,
}

impl PublicError {
    pub fn internal() -> Self {
        Self {
            schema_version: TRANSPORT_SCHEMA_VERSION,
            code: "internal_error".to_owned(),
            message: "Внутренняя ошибка самопроверки.".to_owned(),
        }
    }
}

impl From<&SelfCheckError> for PublicError {
    fn from(error: &SelfCheckError) -> Self {
        let (code, message) = match error {
            SelfCheckError::OpenCvLoad { .. } => (
                "opencv_load_failed",
                "Не удалось загрузить диагностический runtime.",
            ),
            SelfCheckError::MissingBackend { .. } => (
                "opencv_backend_missing",
                "Обязательный backend OpenCV недоступен.",
            ),
            SelfCheckError::ImageCodec { .. } => (
                "image_codec_failed",
                "Проверка JPEG codec завершилась ошибкой.",
            ),
            SelfCheckError::WriterOpen { .. } | SelfCheckError::WriterBackend { .. } => (
                "writer_failed",
                "Проверка video writer завершилась ошибкой.",
            ),
            SelfCheckError::WriterRoundtrip { .. } => (
                "writer_roundtrip_failed",
                "Повторное чтение диагностического видео завершилось ошибкой.",
            ),
            SelfCheckError::RuntimeSupply { .. } | SelfCheckError::Internal { .. } => {
                ("internal_error", "Внутренняя ошибка самопроверки.")
            }
        };

        Self {
            schema_version: TRANSPORT_SCHEMA_VERSION,
            code: code.to_owned(),
            message: message.to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{PublicError, SelfCheckTransportReport};
    use crate::self_check::{AdapterError, LoadedModule, SelfCheckError, SelfCheckReport};

    #[test]
    fn maps_internal_error_without_leaking_source() {
        let internal_marker = r"C:\private\opencv-build\secret.dll";
        let error = SelfCheckError::OpenCvLoad {
            source: AdapterError::new(internal_marker),
        };

        let public = PublicError::from(&error);
        let json = serde_json::to_string(&public).expect("public error serializes");

        assert_eq!(public.code, "opencv_load_failed");
        assert!(!json.contains(internal_marker));
        assert!(!json.contains("source"));
    }

    #[test]
    fn transport_report_preserves_checks_but_omits_internal_module_paths() {
        let internal_marker = r"C:\private\opencv-build\opencv_core4.dll";
        let mut report = SelfCheckReport::empty();
        report.loaded_modules.push(LoadedModule {
            name: "opencv_core4.dll".to_owned(),
            canonical_path: PathBuf::from(internal_marker),
            sha256: "a".repeat(64),
        });

        let transport = SelfCheckTransportReport::from(&report);
        let json = serde_json::to_string(&transport).expect("transport report serializes");

        assert_eq!(transport.checks, report.checks);
        assert!(!json.contains(internal_marker));
        assert!(!json.contains("loaded_modules"));
    }
}
