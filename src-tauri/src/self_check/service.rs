use super::{
    CheckCode, CheckResult, CheckStatus, FileSystemAdapter, ModuleAdapter, OpenCvAdapter,
    SelfCheckOutcome, SelfCheckReport, WriterAdapterError,
};

const REQUIRED_BACKENDS: [&str; 3] = ["MSMF", "DSHOW", "FFMPEG"];

pub struct SelfCheckService<O, M, F> {
    opencv: O,
    modules: M,
    filesystem: F,
}

impl<O, M, F> SelfCheckService<O, M, F>
where
    O: OpenCvAdapter,
    M: ModuleAdapter,
    F: FileSystemAdapter,
{
    pub fn new(opencv: O, modules: M, filesystem: F) -> Self {
        Self {
            opencv,
            modules,
            filesystem,
        }
    }

    pub fn into_adapters(self) -> (O, M, F) {
        (self.opencv, self.modules, self.filesystem)
    }

    pub fn run(&self) -> SelfCheckOutcome {
        let mut report = SelfCheckReport::empty();
        let mut exit_code = 0;

        let summary = match self.opencv.load_summary() {
            Ok(summary) => summary,
            Err(_) => {
                report.checks.push(failed(
                    CheckCode::OpenCvLoad,
                    "Не удалось загрузить OpenCV runtime.",
                ));
                append_skipped_dependents(&mut report);
                return SelfCheckOutcome {
                    report,
                    exit_code: 10,
                };
            }
        };

        let missing_backends: Vec<_> = REQUIRED_BACKENDS
            .iter()
            .filter(|required| {
                !summary
                    .available_backends
                    .iter()
                    .any(|actual| actual.eq_ignore_ascii_case(required))
            })
            .copied()
            .collect();
        if missing_backends.is_empty() {
            report.checks.push(passed(CheckCode::OpenCvLoad));
        } else {
            report.checks.push(failed(
                CheckCode::OpenCvLoad,
                &format!(
                    "Обязательные OpenCV backend недоступны: {}.",
                    missing_backends.join(", ")
                ),
            ));
            exit_code = 11;
        }
        report.opencv = Some(summary);

        match self.opencv.image_codec_roundtrip() {
            Ok(()) => report.checks.push(passed(CheckCode::ImageCodec)),
            Err(_) => {
                report.checks.push(failed(
                    CheckCode::ImageCodec,
                    "JPEG encode/decode round-trip завершился ошибкой.",
                ));
                set_first_failure(&mut exit_code, 12);
            }
        }

        match self.opencv.writer_roundtrip() {
            Ok(writer) if writer.backend_name.eq_ignore_ascii_case("FFMPEG") => {
                report.checks.push(passed(CheckCode::WriterOpen));
                report.checks.push(passed(CheckCode::WriterBackend));
                if writer.frame_count == 30 && writer.width == 320 && writer.height == 240 {
                    report.checks.push(passed(CheckCode::WriterRoundtrip));
                } else {
                    report.checks.push(failed(
                        CheckCode::WriterRoundtrip,
                        "Повторное чтение вернуло неожиданные кадры или размеры.",
                    ));
                    set_first_failure(&mut exit_code, 14);
                }
            }
            Ok(writer) => {
                report.checks.push(passed(CheckCode::WriterOpen));
                report.checks.push(failed(
                    CheckCode::WriterBackend,
                    &format!("Ожидался backend FFMPEG, получен {}.", writer.backend_name),
                ));
                report.checks.push(skipped(CheckCode::WriterRoundtrip));
                set_first_failure(&mut exit_code, 13);
            }
            Err(WriterAdapterError::Open { .. }) => {
                report
                    .checks
                    .push(failed(CheckCode::WriterOpen, "Video writer не открылся."));
                report.checks.push(skipped(CheckCode::WriterBackend));
                report.checks.push(skipped(CheckCode::WriterRoundtrip));
                set_first_failure(&mut exit_code, 13);
            }
            Err(WriterAdapterError::Backend { .. })
            | Err(WriterAdapterError::UnexpectedBackend { .. }) => {
                report.checks.push(passed(CheckCode::WriterOpen));
                report.checks.push(failed(
                    CheckCode::WriterBackend,
                    "Не подтверждён обязательный backend FFMPEG.",
                ));
                report.checks.push(skipped(CheckCode::WriterRoundtrip));
                set_first_failure(&mut exit_code, 13);
            }
            Err(WriterAdapterError::Roundtrip { .. }) => {
                report.checks.push(passed(CheckCode::WriterOpen));
                report.checks.push(passed(CheckCode::WriterBackend));
                report.checks.push(failed(
                    CheckCode::WriterRoundtrip,
                    "Запись или повторное чтение видео завершились ошибкой.",
                ));
                set_first_failure(&mut exit_code, 14);
            }
        }

        match self.modules.loaded_modules() {
            Ok(modules) => report.loaded_modules = modules,
            Err(_) => set_first_failure(&mut exit_code, 10),
        }

        SelfCheckOutcome { report, exit_code }
    }

    pub fn run_and_write(&self, path: &std::path::Path) -> i32 {
        let outcome = self.run();
        match self
            .filesystem
            .write_report_atomically(path, &outcome.report)
        {
            Ok(()) => outcome.exit_code,
            Err(_) => 20,
        }
    }
}

fn passed(check: CheckCode) -> CheckResult {
    CheckResult {
        check,
        status: CheckStatus::Passed,
        public_message: None,
    }
}

fn failed(check: CheckCode, message: &str) -> CheckResult {
    CheckResult {
        check,
        status: CheckStatus::Failed,
        public_message: Some(message.to_owned()),
    }
}

fn skipped(check: CheckCode) -> CheckResult {
    CheckResult {
        check,
        status: CheckStatus::Skipped,
        public_message: Some("Проверка пропущена из-за отказа зависимости.".to_owned()),
    }
}

fn append_skipped_dependents(report: &mut SelfCheckReport) {
    report.checks.extend([
        skipped(CheckCode::ImageCodec),
        skipped(CheckCode::WriterOpen),
        skipped(CheckCode::WriterBackend),
        skipped(CheckCode::WriterRoundtrip),
    ]);
}

fn set_first_failure(current: &mut i32, failure: i32) {
    if *current == 0 {
        *current = failure;
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, path::Path};

    use super::SelfCheckService;
    use crate::self_check::{
        AdapterError, CheckCode, CheckStatus, FileSystemAdapter, LoadedModule, ModuleAdapter,
        OpenCvAdapter, OpenCvSummary, ReportWriteError, SelfCheckReport, WriterAdapterError,
        WriterRoundtripSummary,
    };

    enum LoadBehavior {
        Success(Vec<String>),
        Failure,
    }

    enum WriterBehavior {
        Success(WriterRoundtripSummary),
        OpenFailure,
        BackendFailure,
        RoundtripFailure,
    }

    struct FakeOpenCv {
        load: LoadBehavior,
        image_succeeds: bool,
        writer: WriterBehavior,
    }

    impl OpenCvAdapter for FakeOpenCv {
        fn load_summary(&self) -> Result<OpenCvSummary, AdapterError> {
            match &self.load {
                LoadBehavior::Success(backends) => Ok(OpenCvSummary {
                    version: "4.12.0".to_owned(),
                    build_information_sha256: "a".repeat(64),
                    available_backends: backends.clone(),
                }),
                LoadBehavior::Failure => Err(AdapterError::new("fixture load failure")),
            }
        }

        fn image_codec_roundtrip(&self) -> Result<(), AdapterError> {
            self.image_succeeds
                .then_some(())
                .ok_or_else(|| AdapterError::new("fixture codec failure"))
        }

        fn writer_roundtrip(&self) -> Result<WriterRoundtripSummary, WriterAdapterError> {
            match &self.writer {
                WriterBehavior::Success(summary) => Ok(summary.clone()),
                WriterBehavior::OpenFailure => Err(WriterAdapterError::Open {
                    source: AdapterError::new("fixture open failure"),
                }),
                WriterBehavior::BackendFailure => Err(WriterAdapterError::Backend {
                    source: AdapterError::new("fixture backend failure"),
                }),
                WriterBehavior::RoundtripFailure => Err(WriterAdapterError::Roundtrip {
                    source: AdapterError::new("fixture roundtrip failure"),
                }),
            }
        }
    }

    struct FakeModules(bool);

    impl ModuleAdapter for FakeModules {
        fn loaded_modules(&self) -> Result<Vec<LoadedModule>, AdapterError> {
            self.0
                .then_some(Vec::new())
                .ok_or_else(|| AdapterError::new("fixture module failure"))
        }
    }

    struct FakeFileSystem;

    impl FileSystemAdapter for FakeFileSystem {
        fn write_report_atomically(
            &self,
            _path: &Path,
            _report: &SelfCheckReport,
        ) -> Result<(), ReportWriteError> {
            Ok(())
        }
    }

    fn service(
        load: LoadBehavior,
        image_succeeds: bool,
        writer: WriterBehavior,
        modules_succeed: bool,
    ) -> SelfCheckService<FakeOpenCv, FakeModules, FakeFileSystem> {
        SelfCheckService::new(
            FakeOpenCv {
                load,
                image_succeeds,
                writer,
            },
            FakeModules(modules_succeed),
            FakeFileSystem,
        )
    }

    fn success_writer() -> WriterBehavior {
        WriterBehavior::Success(WriterRoundtripSummary {
            backend_name: "FFMPEG".to_owned(),
            frame_count: 30,
            width: 320,
            height: 240,
        })
    }

    fn status_map(
        outcome: &crate::self_check::SelfCheckOutcome,
    ) -> HashMap<CheckCode, CheckStatus> {
        outcome
            .report
            .checks
            .iter()
            .map(|result| (result.check, result.status))
            .collect()
    }

    fn all_backends() -> LoadBehavior {
        LoadBehavior::Success(vec![
            "MSMF".to_owned(),
            "DSHOW".to_owned(),
            "FFMPEG".to_owned(),
        ])
    }

    #[test]
    fn reports_all_successful_checks() {
        let outcome = service(all_backends(), true, success_writer(), true).run();

        assert_eq!(outcome.exit_code, 0);
        assert!(
            outcome
                .report
                .checks
                .iter()
                .all(|result| result.status == CheckStatus::Passed)
        );
    }

    #[test]
    fn skips_all_dependents_when_opencv_load_fails() {
        let outcome = service(LoadBehavior::Failure, true, success_writer(), true).run();
        let statuses = status_map(&outcome);

        assert_eq!(outcome.exit_code, 10);
        assert_eq!(statuses[&CheckCode::OpenCvLoad], CheckStatus::Failed);
        assert_eq!(statuses[&CheckCode::ImageCodec], CheckStatus::Skipped);
        assert_eq!(statuses[&CheckCode::WriterOpen], CheckStatus::Skipped);
    }

    #[test]
    fn missing_backend_does_not_skip_independent_checks() {
        let outcome = service(
            LoadBehavior::Success(vec!["FFMPEG".to_owned()]),
            true,
            success_writer(),
            true,
        )
        .run();
        let statuses = status_map(&outcome);

        assert_eq!(outcome.exit_code, 11);
        assert_eq!(statuses[&CheckCode::OpenCvLoad], CheckStatus::Failed);
        assert_eq!(statuses[&CheckCode::ImageCodec], CheckStatus::Passed);
        assert_eq!(statuses[&CheckCode::WriterRoundtrip], CheckStatus::Passed);
    }

    #[test]
    fn image_failure_preserves_writer_results() {
        let outcome = service(all_backends(), false, success_writer(), true).run();
        let statuses = status_map(&outcome);

        assert_eq!(outcome.exit_code, 12);
        assert_eq!(statuses[&CheckCode::ImageCodec], CheckStatus::Failed);
        assert_eq!(statuses[&CheckCode::WriterRoundtrip], CheckStatus::Passed);
    }

    #[test]
    fn writer_open_failure_skips_backend_and_roundtrip() {
        let outcome = service(all_backends(), true, WriterBehavior::OpenFailure, true).run();
        let statuses = status_map(&outcome);

        assert_eq!(outcome.exit_code, 13);
        assert_eq!(statuses[&CheckCode::WriterOpen], CheckStatus::Failed);
        assert_eq!(statuses[&CheckCode::WriterBackend], CheckStatus::Skipped);
        assert_eq!(statuses[&CheckCode::WriterRoundtrip], CheckStatus::Skipped);
    }

    #[test]
    fn writer_backend_failure_skips_roundtrip() {
        let outcome = service(all_backends(), true, WriterBehavior::BackendFailure, true).run();
        let statuses = status_map(&outcome);

        assert_eq!(outcome.exit_code, 13);
        assert_eq!(statuses[&CheckCode::WriterOpen], CheckStatus::Passed);
        assert_eq!(statuses[&CheckCode::WriterBackend], CheckStatus::Failed);
        assert_eq!(statuses[&CheckCode::WriterRoundtrip], CheckStatus::Skipped);
    }

    #[test]
    fn unexpected_writer_backend_is_rejected() {
        let outcome = service(
            all_backends(),
            true,
            WriterBehavior::Success(WriterRoundtripSummary {
                backend_name: "MSMF".to_owned(),
                frame_count: 30,
                width: 320,
                height: 240,
            }),
            true,
        )
        .run();
        let statuses = status_map(&outcome);

        assert_eq!(outcome.exit_code, 13);
        assert_eq!(statuses[&CheckCode::WriterBackend], CheckStatus::Failed);
        assert_eq!(statuses[&CheckCode::WriterRoundtrip], CheckStatus::Skipped);
    }

    #[test]
    fn writer_roundtrip_failure_is_distinct() {
        let outcome = service(all_backends(), true, WriterBehavior::RoundtripFailure, true).run();
        let statuses = status_map(&outcome);

        assert_eq!(outcome.exit_code, 14);
        assert_eq!(statuses[&CheckCode::WriterBackend], CheckStatus::Passed);
        assert_eq!(statuses[&CheckCode::WriterRoundtrip], CheckStatus::Failed);
    }

    #[test]
    fn writer_count_or_dimensions_mismatch_is_rejected() {
        let outcome = service(
            all_backends(),
            true,
            WriterBehavior::Success(WriterRoundtripSummary {
                backend_name: "FFMPEG".to_owned(),
                frame_count: 29,
                width: 320,
                height: 240,
            }),
            true,
        )
        .run();
        let statuses = status_map(&outcome);

        assert_eq!(outcome.exit_code, 14);
        assert_eq!(statuses[&CheckCode::WriterRoundtrip], CheckStatus::Failed);
    }

    #[test]
    fn module_failure_changes_successful_run_to_runtime_failure() {
        let outcome = service(all_backends(), true, success_writer(), false).run();

        assert_eq!(outcome.exit_code, 10);
    }
}
