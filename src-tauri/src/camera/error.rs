use std::error::Error as StdError;

use thiserror::Error;

use crate::camera::profiling::ProfileConfigError;

#[derive(Debug, Error, Eq, PartialEq)]
pub enum CameraConfigError {
    #[error("unsupported camera transport schema version")]
    UnsupportedSchema,
    #[error("camera request shape is invalid")]
    InvalidRequestShape,
    #[error("first index {first_index} is greater than last index {last_index}")]
    ReversedRange { first_index: u32, last_index: u32 },
    #[error("camera index range exceeds the limit of {limit}")]
    RangeTooLarge { limit: u32 },
    #[error("at least one camera backend is required")]
    EmptyBackends,
    #[error("camera backend list contains a duplicate")]
    DuplicateBackend,
    #[error("unsupported camera backend")]
    UnknownBackend,
    #[error("camera probe count exceeds the limit of {limit}")]
    TooManyProbes { limit: usize },
    #[error("{field} must be within 1..={maximum_ms}")]
    InvalidDuration {
        field: &'static str,
        maximum_ms: u64,
    },
    #[error("camera deadlines are inconsistent: {message}")]
    InconsistentDurations { message: &'static str },
}

pub type CameraConfigResult<T> = Result<T, CameraConfigError>;

#[derive(Debug, Error)]
pub enum CaptureAdapterError {
    #[error("camera open failed")]
    Open {
        #[source]
        source: Box<dyn StdError + Send + Sync>,
    },
    #[error("camera open-state query failed")]
    OpenState {
        #[source]
        source: Box<dyn StdError + Send + Sync>,
    },
    #[error("camera mode apply failed")]
    Apply {
        #[source]
        source: Box<dyn StdError + Send + Sync>,
    },
    #[error("camera reported-property query failed")]
    Get {
        #[source]
        source: Box<dyn StdError + Send + Sync>,
    },
    #[error("camera read failed")]
    Read {
        #[source]
        source: Box<dyn StdError + Send + Sync>,
    },
    #[error("camera release failed")]
    Release {
        #[source]
        source: Box<dyn StdError + Send + Sync>,
    },
}

impl CaptureAdapterError {
    pub fn open(source: impl StdError + Send + Sync + 'static) -> Self {
        Self::Open {
            source: Box::new(source),
        }
    }

    pub fn open_state(source: impl StdError + Send + Sync + 'static) -> Self {
        Self::OpenState {
            source: Box::new(source),
        }
    }

    pub fn read(source: impl StdError + Send + Sync + 'static) -> Self {
        Self::Read {
            source: Box::new(source),
        }
    }

    pub fn apply(source: impl StdError + Send + Sync + 'static) -> Self {
        Self::Apply {
            source: Box::new(source),
        }
    }

    pub fn get(source: impl StdError + Send + Sync + 'static) -> Self {
        Self::Get {
            source: Box::new(source),
        }
    }

    pub fn release(source: impl StdError + Send + Sync + 'static) -> Self {
        Self::Release {
            source: Box::new(source),
        }
    }
}

pub type CaptureAdapterResult<T> = Result<T, CaptureAdapterError>;

#[derive(Debug, Error)]
pub enum CameraServiceError {
    #[error(transparent)]
    InvalidConfig(#[from] CameraConfigError),
    #[error(transparent)]
    InvalidProfileConfig(#[from] ProfileConfigError),
    #[error("camera service is busy")]
    Busy,
    #[error("camera service is stuck and requires process restart")]
    Stuck,
    #[error("camera scan operation is stale")]
    StaleOperation,
    #[error("camera endpoint is stale")]
    StaleDeviceEndpoint,
    #[error("camera profile operation is stale")]
    StaleProfileOperation,
    #[error("camera profile result is not ready")]
    ProfileNotReady,
    #[error("camera scan generation is exhausted")]
    GenerationExhausted,
    #[error("camera worker could not be started")]
    WorkerStart {
        #[source]
        source: std::io::Error,
    },
    #[error("camera synchronization failed")]
    Synchronization,
    #[error("camera worker failed")]
    Worker {
        #[source]
        source: CaptureAdapterError,
    },
    #[error("camera worker panicked")]
    WorkerPanicked,
}

pub type CameraServiceResult<T> = Result<T, CameraServiceError>;

#[cfg(test)]
mod tests {
    use std::{error::Error, io};

    use super::*;

    #[test]
    fn adapter_error_preserves_internal_source_chain() {
        for error in [
            CaptureAdapterError::apply(io::Error::other("APPLY_MARKER")),
            CaptureAdapterError::get(io::Error::other("GET_MARKER")),
            CaptureAdapterError::read(io::Error::other("READ_MARKER")),
            CaptureAdapterError::release(io::Error::other("RELEASE_MARKER")),
        ] {
            assert!(
                error
                    .source()
                    .is_some_and(|source| source.to_string().ends_with("MARKER"))
            );
        }
    }

    #[test]
    fn service_error_keeps_expected_variants_separate() {
        assert!(matches!(CameraServiceError::Busy, CameraServiceError::Busy));
        assert!(matches!(
            CameraServiceError::from(CameraConfigError::UnknownBackend),
            CameraServiceError::InvalidConfig(CameraConfigError::UnknownBackend)
        ));
    }
}
