pub mod cli;
pub mod rules;

#[allow(dead_code)] // Parser foundation is consumed by the project graph in MAT-15.
pub(crate) mod python;

#[cfg(test)]
mod python_tests;

use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io::Write;
use std::path::{Path, PathBuf};

use cli::{Cli, Command};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExitStatus {
    Success,
    Violations,
}

impl ExitStatus {
    pub const fn code(self) -> u8 {
        match self {
            Self::Success => 0,
            Self::Violations => 1,
        }
    }

    pub const fn from_diagnostic_count(count: usize) -> Self {
        if count == 0 {
            Self::Success
        } else {
            Self::Violations
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum CliError {
    MissingPath(PathBuf),
    WriteOutput,
}

impl Display for CliError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingPath(path) => write!(formatter, "path does not exist: {}", path.display()),
            Self::WriteOutput => formatter.write_str("failed to write command output"),
        }
    }
}

impl Error for CliError {}

pub fn execute(cli: Cli, output: &mut dyn Write) -> Result<ExitStatus, CliError> {
    match cli.command {
        Command::Check { paths } => check(paths),
        Command::Rule { code } => explain_rule(code, output),
        Command::Config => show_config(output),
    }
}

fn check(paths: Vec<PathBuf>) -> Result<ExitStatus, CliError> {
    if paths.is_empty() {
        ensure_path_exists(Path::new("."))?;
    } else {
        for path in paths {
            ensure_path_exists(&path)?;
        }
    }

    // V1 enables no rules unless configuration selects them. Configuration and
    // analysis are introduced by later implementation tasks.
    Ok(ExitStatus::Success)
}

fn ensure_path_exists(path: &Path) -> Result<(), CliError> {
    if path.exists() {
        Ok(())
    } else {
        Err(CliError::MissingPath(path.to_owned()))
    }
}

fn explain_rule(code: rules::RuleCode, output: &mut dyn Write) -> Result<ExitStatus, CliError> {
    let rule = code.summary();
    writeln!(output, "{}: {}", rule.code, rule.name).map_err(|_| CliError::WriteOutput)?;
    writeln!(output, "Documentation: {}", rule.documentation).map_err(|_| CliError::WriteOutput)?;
    Ok(ExitStatus::Success)
}

fn show_config(output: &mut dyn Write) -> Result<ExitStatus, CliError> {
    writeln!(output, "# Configuration discovery is not implemented yet.")
        .map_err(|_| CliError::WriteOutput)?;
    writeln!(output, "select = []").map_err(|_| CliError::WriteOutput)?;
    Ok(ExitStatus::Success)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::cli::{Cli, Command};

    use super::{CliError, ExitStatus, execute};

    #[test]
    fn diagnostic_count_maps_to_documented_exit_status() {
        assert_eq!(ExitStatus::from_diagnostic_count(0).code(), 0);
        assert_eq!(ExitStatus::from_diagnostic_count(1).code(), 1);
    }

    #[test]
    fn missing_check_path_is_an_error() {
        let cli = Cli {
            verbose: 0,
            command: Command::Check {
                paths: vec![PathBuf::from("path-that-cannot-exist")],
            },
        };

        assert_eq!(
            execute(cli, &mut Vec::new()),
            Err(CliError::MissingPath(PathBuf::from(
                "path-that-cannot-exist"
            )))
        );
    }

    #[test]
    fn config_shows_no_selected_rules() {
        let cli = Cli {
            verbose: 0,
            command: Command::Config,
        };
        let mut output = Vec::new();

        let status = execute(cli, &mut output).expect("config should succeed");

        assert_eq!(status, ExitStatus::Success);
        assert_eq!(
            output,
            b"# Configuration discovery is not implemented yet.\nselect = []\n"
        );
    }
}
