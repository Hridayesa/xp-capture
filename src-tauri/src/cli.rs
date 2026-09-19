use std::{ffi::OsString, path::PathBuf};

#[derive(Debug, Eq, PartialEq)]
pub enum StartupMode {
    Gui,
    SelfCheck { report_path: PathBuf },
}

pub fn parse_startup_mode(
    arguments: impl IntoIterator<Item = OsString>,
) -> Result<StartupMode, String> {
    let arguments = arguments.into_iter().collect::<Vec<_>>();
    match arguments.as_slice() {
        [] => Ok(StartupMode::Gui),
        [self_check, json, path]
            if self_check == "--self-check" && json == "--json" && !path.is_empty() =>
        {
            Ok(StartupMode::SelfCheck {
                report_path: PathBuf::from(path),
            })
        }
        _ => Err("Использование: xp-capture --self-check --json <path>".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::{StartupMode, parse_startup_mode};

    #[test]
    fn parses_gui_and_headless_modes() {
        assert_eq!(parse_startup_mode(Vec::new()), Ok(StartupMode::Gui));
        assert_eq!(
            parse_startup_mode([
                OsString::from("--self-check"),
                OsString::from("--json"),
                OsString::from("report.json"),
            ]),
            Ok(StartupMode::SelfCheck {
                report_path: "report.json".into(),
            })
        );
    }

    #[test]
    fn rejects_unknown_or_incomplete_arguments() {
        assert!(parse_startup_mode([OsString::from("--self-check")]).is_err());
        assert!(parse_startup_mode([OsString::from("--unknown")]).is_err());
    }
}
