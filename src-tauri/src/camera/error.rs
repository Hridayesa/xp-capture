use std::error::Error as StdError;

use thiserror::Error;

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
    #[error("camera service is busy")]
    Busy,
    #[error("camera service is stuck and requires process restart")]
    Stuck,
    #[error("camera scan operation is stale")]
    StaleOperation,
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
        let error = CaptureAdapterError::read(io::Error::other("NATIVE_MARKER"));
        assert_eq!(
            error.source().map(ToString::to_string).as_deref(),
            Some("NATIVE_MARKER")
        );
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
