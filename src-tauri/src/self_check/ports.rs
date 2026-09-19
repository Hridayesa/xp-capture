use std::path::Path;

use super::{
    AdapterError, LoadedModule, OpenCvSummary, ReportWriteError, SelfCheckReport,
    WriterAdapterError,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WriterRoundtripSummary {
    pub backend_name: String,
    pub frame_count: u32,
    pub width: u32,
    pub height: u32,
}

pub trait OpenCvAdapter: Send + Sync {
    fn load_summary(&self) -> Result<OpenCvSummary, AdapterError>;
    fn image_codec_roundtrip(&self) -> Result<(), AdapterError>;
    fn writer_roundtrip(&self) -> Result<WriterRoundtripSummary, WriterAdapterError>;
}

pub trait ModuleAdapter: Send + Sync {
    fn loaded_modules(&self) -> Result<Vec<LoadedModule>, AdapterError>;
}

pub trait FileSystemAdapter: Send + Sync {
    fn write_report_atomically(
        &self,
        path: &Path,
        report: &SelfCheckReport,
    ) -> Result<(), ReportWriteError>;
}
