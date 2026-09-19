pub mod adapters;
mod error;
mod model;
mod ports;
mod profile_worker;
pub mod profiling;
mod service;
pub mod transport;
mod worker;

pub use adapters::OpenCvCaptureAdapterFactory;
pub use error::{
    CameraConfigError, CameraConfigResult, CameraServiceError, CameraServiceResult,
    CaptureAdapterError, CaptureAdapterResult,
};
pub use model::{
    CameraFailureCode, CameraServiceSnapshot, CameraServiceState, CaptureBackend,
    DEFAULT_FIRST_FRAME_DEADLINE_MS, DEFAULT_FIRST_INDEX, DEFAULT_LAST_INDEX,
    DEFAULT_OPERATION_DEADLINE_MS, DEFAULT_REOPEN_DELAY_MS, DEFAULT_SHUTDOWN_DEADLINE_MS,
    DeviceEndpoint, DeviceEndpointKey, DeviceScanPolicy, DeviceScanSnapshot, DeviceScanStatus,
    MAX_DURATION_MS, MAX_INDICES, MAX_PROBES, OperationId, ProbeOutcome, ProbeStatus, ProbeTarget,
};
pub use ports::{
    CaptureAdapter, CaptureAdapterFactory, CaptureOpen, CaptureSession, FrameRead, MonotonicClock,
    SystemMonotonicClock,
};
pub use service::CameraService;
pub use transport::{
    CAMERA_TRANSPORT_SCHEMA_VERSION, CameraPublicErrorV1, CameraServiceSnapshotV1,
    DeviceScanOperationRequestV1, DeviceScanPolicyV1, DeviceScanRequestV1, DeviceScanSnapshotV1,
    DeviceScanStartedV1, ProfileProgressV1, ProfileReportV1, ProfileStartedV1,
    cancel_device_scan_for, cancel_profile_for, get_device_scan_for, get_profile_result_for,
    get_profile_status_for, start_device_scan_for, start_profile_for, stop_camera_for,
};
