use std::{io, path::PathBuf};

use thiserror::Error;

#[derive(Debug, Error)]
#[error("native adapter operation failed: {message}")]
pub struct AdapterError {
    message: String,
    #[source]
    source: Option<Box<dyn std::error::Error + Send + Sync>>,
}

#[derive(Debug, Error)]
pub enum WriterAdapterError {
    #[error("video writer failed to open")]
    Open {
        #[source]
        source: AdapterError,
    },
    #[error("video writer backend could not be read")]
    Backend {
        #[source]
        source: AdapterError,
    },
    #[error("unexpected video writer backend: {actual}")]
    UnexpectedBackend { actual: String },
    #[error("video writer round-trip failed")]
    Roundtrip {
        #[source]
        source: AdapterError,
    },
}

impl AdapterError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            source: None,
        }
    }

    pub fn with_source(
        message: impl Into<String>,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            message: message.into(),
            source: Some(Box::new(source)),
        }
    }
}

#[derive(Debug, Error)]
pub enum RuntimeSupplyError {
    #[error("runtime manifest is invalid: {reason}")]
    InvalidManifest { reason: String },
    #[error("failed to read runtime manifest at {path}")]
    ManifestRead {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to parse runtime manifest at {path}")]
    ManifestParse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("runtime module is not allowlisted: {module}")]
    ModuleNotAllowed { module: String },
    #[error("runtime module hash mismatch: {module}")]
    HashMismatch { module: String },
    #[error("runtime module origin is not allowed: {module}")]
    InvalidOrigin { module: String },
}

#[derive(Debug, Error)]
pub enum SelfCheckError {
    #[error("OpenCV could not be loaded")]
    OpenCvLoad {
        #[source]
        source: AdapterError,
    },
    #[error("required OpenCV backend is unavailable: {backend}")]
    MissingBackend { backend: String },
    #[error("image codec round-trip failed")]
    ImageCodec {
        #[source]
        source: AdapterError,
    },
    #[error("video writer failed to open or finalize")]
    WriterOpen {
        #[source]
        source: AdapterError,
    },
    #[error("unexpected video writer backend: {actual}")]
    WriterBackend { actual: String },
    #[error("video writer round-trip failed")]
    WriterRoundtrip {
        #[source]
        source: AdapterError,
    },
    #[error("runtime module provenance validation failed")]
    RuntimeSupply {
        #[source]
        source: RuntimeSupplyError,
    },
    #[error("internal self-check failure")]
    Internal {
        #[source]
        source: AdapterError,
    },
}

impl SelfCheckError {
    pub const fn exit_code(&self) -> i32 {
        match self {
            Self::OpenCvLoad { .. } | Self::RuntimeSupply { .. } => 10,
            Self::MissingBackend { .. } => 11,
            Self::ImageCodec { .. } => 12,
            Self::WriterOpen { .. } | Self::WriterBackend { .. } => 13,
            Self::WriterRoundtrip { .. } => 14,
            Self::Internal { .. } => 20,
        }
    }
}

#[derive(Debug, Error)]
pub enum ReportWriteError {
    #[error("failed to create report directory")]
    CreateDirectory {
        #[source]
        source: io::Error,
    },
    #[error("failed to serialize self-check report")]
    Serialize {
        #[source]
        source: serde_json::Error,
    },
    #[error("failed to write self-check report")]
    Write {
        #[source]
        source: io::Error,
    },
    #[error("failed to atomically replace self-check report")]
    Replace {
        #[source]
        source: io::Error,
    },
}

impl ReportWriteError {
    pub const fn exit_code(&self) -> i32 {
        20
    }
}

#[cfg(test)]
mod tests {
    use super::{AdapterError, ReportWriteError, SelfCheckError};

    #[test]
    fn maps_failures_to_stable_exit_codes() {
        let cases = [
            (
                SelfCheckError::OpenCvLoad {
                    source: AdapterError::new("load"),
                },
                10,
            ),
            (
                SelfCheckError::MissingBackend {
                    backend: "FFMPEG".to_owned(),
                },
                11,
            ),
            (
                SelfCheckError::ImageCodec {
                    source: AdapterError::new("codec"),
                },
                12,
            ),
            (
                SelfCheckError::WriterOpen {
                    source: AdapterError::new("open"),
                },
                13,
            ),
            (
                SelfCheckError::WriterRoundtrip {
                    source: AdapterError::new("reread"),
                },
                14,
            ),
            (
                SelfCheckError::Internal {
                    source: AdapterError::new("internal"),
                },
                20,
            ),
        ];

        for (error, expected) in cases {
            assert_eq!(error.exit_code(), expected);
        }

        let report_error = ReportWriteError::Write {
            source: std::io::Error::other("disk"),
        };
        assert_eq!(report_error.exit_code(), 20);
    }
}
