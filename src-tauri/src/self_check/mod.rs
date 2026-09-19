mod adapters;
mod error;
mod model;
mod ports;
mod runtime_manifest;
mod service;
mod transport;

pub use adapters::{AtomicFileSystemAdapter, OpenCvRuntimeAdapter, WindowsModuleAdapter};
pub use error::{
    AdapterError, ReportWriteError, RuntimeSupplyError, SelfCheckError, WriterAdapterError,
};
pub use model::{
    CheckCode, CheckResult, CheckStatus, LoadedModule, OpenCvSummary, SELF_CHECK_SCHEMA_VERSION,
    SelfCheckOutcome, SelfCheckReport,
};
pub use ports::{FileSystemAdapter, ModuleAdapter, OpenCvAdapter, WriterRoundtripSummary};
pub use runtime_manifest::{
    PeArchitecture, RUNTIME_MANIFEST_SCHEMA_VERSION, RuntimeFilePurpose, RuntimeManifest,
    RuntimeManifestFile,
};
pub use service::SelfCheckService;
pub use transport::{PublicError, SelfCheckTransportReport, TRANSPORT_SCHEMA_VERSION};
