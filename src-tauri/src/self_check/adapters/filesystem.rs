use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicU64, Ordering},
};

use crate::self_check::{FileSystemAdapter, ReportWriteError, SelfCheckReport};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Default)]
pub struct AtomicFileSystemAdapter;

impl AtomicFileSystemAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl FileSystemAdapter for AtomicFileSystemAdapter {
    fn write_report_atomically(
        &self,
        path: &Path,
        report: &SelfCheckReport,
    ) -> Result<(), ReportWriteError> {
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty());
        if let Some(parent) = parent {
            fs::create_dir_all(parent)
                .map_err(|source| ReportWriteError::CreateDirectory { source })?;
        }
        let bytes = serde_json::to_vec_pretty(report)
            .map_err(|source| ReportWriteError::Serialize { source })?;
        let temporary = temporary_sibling(path);
        let mut cleanup = TemporaryReport::new(temporary.clone());
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|source| ReportWriteError::Write { source })?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|source| ReportWriteError::Write { source })?;
        drop(file);
        replace_file(&temporary, path).map_err(|source| ReportWriteError::Replace { source })?;
        cleanup.disarm();
        Ok(())
    }
}

fn temporary_sibling(path: &Path) -> PathBuf {
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("self-check.json");
    path.with_file_name(format!(".{file_name}.{}.{}.tmp", process::id(), sequence))
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let succeeded = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if succeeded == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::rename(source, destination)
}

struct TemporaryReport {
    path: Option<PathBuf>,
}

impl TemporaryReport {
    fn new(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    fn disarm(&mut self) {
        self.path = None;
    }
}

impl Drop for TemporaryReport {
    fn drop(&mut self) {
        if let Some(path) = &self.path {
            let _ = fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, sync::atomic::AtomicU64};

    use super::{AtomicFileSystemAdapter, TEMP_SEQUENCE};
    use crate::self_check::{FileSystemAdapter, SelfCheckReport};

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn writes_and_replaces_report_without_leaving_temporary_file() {
        let root = std::env::temp_dir().join(format!(
            "xp-capture-report-test-{}-{}",
            std::process::id(),
            TEST_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let report_path = root.join("nested").join("report.json");
        let adapter = AtomicFileSystemAdapter::new();
        adapter
            .write_report_atomically(&report_path, &SelfCheckReport::empty())
            .expect("first report write succeeds");
        adapter
            .write_report_atomically(&report_path, &SelfCheckReport::empty())
            .expect("existing report is atomically replaced");

        let value: serde_json::Value =
            serde_json::from_slice(&fs::read(&report_path).expect("written report can be read"))
                .expect("written report is valid JSON");
        assert_eq!(value["schema_version"], 1);
        assert_eq!(
            fs::read_dir(report_path.parent().expect("report has parent"))
                .expect("report directory can be listed")
                .count(),
            1
        );

        let _ = fs::remove_dir_all(root);
        let _ = TEMP_SEQUENCE.load(std::sync::atomic::Ordering::Relaxed);
    }
}
