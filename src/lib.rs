pub mod cli;
pub mod rules;

pub(crate) mod config;
pub(crate) mod ct;
pub(crate) mod di;
pub mod diagnostics;
pub(crate) mod dm;
#[allow(dead_code)] // Semantic graph queries are consumed by rules in MAT-7 and MAT-9.
pub(crate) mod graph;
#[allow(dead_code)] // Project metadata is consumed by graph indexing in MAT-15.
pub(crate) mod project;
pub(crate) mod suppression;

#[allow(dead_code)] // Parser foundation is consumed by the project graph in MAT-15.
pub(crate) mod python;

#[cfg(test)]
mod python_tests;

use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io::Write;
use std::path::{Path, PathBuf};

use cli::{Cli, Command, OutputFormat};

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
    CurrentDirectory,
    Configuration(String),
    MissingPath(PathBuf),
    WriteOutput,
}

impl Display for CliError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::CurrentDirectory => formatter.write_str("failed to resolve current directory"),
            Self::Configuration(message) => formatter.write_str(message),
            Self::MissingPath(path) => write!(formatter, "path does not exist: {}", path.display()),
            Self::WriteOutput => formatter.write_str("failed to write command output"),
        }
    }
}

impl Error for CliError {}

pub fn execute(cli: Cli, output: &mut dyn Write) -> Result<ExitStatus, CliError> {
    let current_dir = std::env::current_dir().map_err(|_| CliError::CurrentDirectory)?;
    execute_from(cli, output, &current_dir, false)
}

pub fn execute_styled(cli: Cli, output: &mut dyn Write) -> Result<ExitStatus, CliError> {
    let current_dir = std::env::current_dir().map_err(|_| CliError::CurrentDirectory)?;
    execute_from(cli, output, &current_dir, true)
}

fn execute_from(
    cli: Cli,
    output: &mut dyn Write,
    current_dir: &Path,
    styled: bool,
) -> Result<ExitStatus, CliError> {
    let config_path = cli.config.as_deref();
    match cli.command {
        Command::Check {
            paths,
            output_format,
        } => check(
            paths,
            output_format,
            current_dir,
            config_path,
            output,
            styled,
        ),
        Command::Rule { code } => explain_rule(code, output),
        Command::Config => show_config(output, current_dir, config_path),
    }
}

fn check(
    paths: Vec<PathBuf>,
    output_format: OutputFormat,
    current_dir: &Path,
    config_path: Option<&Path>,
    output: &mut dyn Write,
    styled: bool,
) -> Result<ExitStatus, CliError> {
    if paths.is_empty() {
        ensure_path_exists(current_dir)?;
    } else {
        for path in &paths {
            let path = if path.is_absolute() {
                path.clone()
            } else {
                current_dir.join(path)
            };
            ensure_path_exists(&path)?;
        }
    }

    let project = project::Project::resolve(current_dir, config_path)
        .map_err(|error| CliError::Configuration(error.to_string()))?;
    let discovered = project
        .discover_files(current_dir, &paths)
        .map_err(|error| CliError::Configuration(error.to_string()))?;
    let build = graph::ProjectGraph::build(&project, &discovered)
        .map_err(|error| CliError::Configuration(error.to_string()))?;
    let mut sources = diagnostics::SourceMap::default();
    for file in build.files() {
        if let Some(source) = &file.source {
            sources.insert(source.clone());
        }
    }
    if let Some(config_path) = project.config_path() {
        let source = std::fs::read_to_string(config_path).map_err(|error| {
            CliError::Configuration(format!(
                "failed to read configuration {}: {error}",
                config_path.display()
            ))
        })?;
        sources.insert(python::SourceFile::new(config_path, source));
    }
    let (exceptions, directive_errors) =
        suppression::DesignExceptions::parse(build.files(), &project.config().per_file_ignores);
    if !build.syntax_errors().is_empty() || !directive_errors.is_empty() {
        match output_format {
            OutputFormat::Human => diagnostics::write_source_findings_human(
                output,
                project.root(),
                &sources,
                &directive_errors,
                build.syntax_errors(),
            ),
            OutputFormat::Json => diagnostics::write_source_findings_json(
                output,
                project.root(),
                &sources,
                &directive_errors,
                build.syntax_errors(),
            ),
        }
        .map_err(|_| CliError::WriteOutput)?;
        return Ok(ExitStatus::Violations);
    }

    let analyzed_files = build
        .files()
        .iter()
        .filter(|file| file.diagnostic)
        .map(|file| file.path.clone())
        .collect::<Vec<_>>();
    let graph = build
        .into_rule_graph()
        .expect("syntax errors were checked before rule execution");

    // Keeping rule execution below the syntax/directive guard prevents partial analysis.
    let mut rule_diagnostics = ct::run(&graph, &project.config().enabled_rules);
    rule_diagnostics.extend(di::run(&graph, &project.config().enabled_rules));
    rule_diagnostics.extend(dm::run(
        &graph,
        &project.config().enabled_rules,
        &project.config().dm002,
    ));
    let diagnostics = exceptions.apply_with_hygiene(
        rule_diagnostics,
        project.root(),
        &project.config().enabled_rules,
        &analyzed_files,
        project.config_path(),
    );
    match output_format {
        OutputFormat::Human => {
            diagnostics.write_human_styled(output, project.root(), &sources, styled)
        }
        OutputFormat::Json => diagnostics.write_json(output, project.root(), &sources),
    }
    .map_err(|_| CliError::WriteOutput)?;
    Ok(ExitStatus::from_diagnostic_count(
        diagnostics.as_slice().len(),
    ))
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
    writeln!(
        output,
        "Family: {} ({})",
        rule.family.name, rule.family.selector
    )
    .map_err(|_| CliError::WriteOutput)?;
    if rule.configuration.is_empty() {
        writeln!(output, "Configuration: none").map_err(|_| CliError::WriteOutput)?;
    } else {
        writeln!(output, "Configuration:").map_err(|_| CliError::WriteOutput)?;
        for field in rule.configuration {
            writeln!(output, "  {} = {}", field.name, field.default)
                .map_err(|_| CliError::WriteOutput)?;
        }
    }
    writeln!(output, "Documentation: {}", rule.documentation).map_err(|_| CliError::WriteOutput)?;
    Ok(ExitStatus::Success)
}

fn show_config(
    output: &mut dyn Write,
    current_dir: &Path,
    config_path: Option<&Path>,
) -> Result<ExitStatus, CliError> {
    let project = project::Project::resolve(current_dir, config_path)
        .map_err(|error| CliError::Configuration(error.to_string()))?;
    project
        .config()
        .write(output)
        .map_err(|_| CliError::WriteOutput)?;
    Ok(ExitStatus::Success)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::cli::{Cli, Command, OutputFormat};

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
            config: None,
            command: Command::Check {
                output_format: OutputFormat::Human,
                paths: vec![PathBuf::from("path-that-cannot-exist")],
            },
        };

        assert_eq!(
            execute(cli, &mut Vec::new()),
            Err(CliError::MissingPath(
                std::env::current_dir()
                    .expect("current directory")
                    .join("path-that-cannot-exist")
            ))
        );
    }

    #[test]
    fn config_shows_no_selected_rules() {
        let cli = Cli {
            verbose: 0,
            config: None,
            command: Command::Config,
        };
        let mut output = Vec::new();

        let status = execute(cli, &mut output).expect("config should succeed");

        assert_eq!(status, ExitStatus::Success);
        assert_eq!(
            output,
            b"select = []\nignore = []\nenabled-rules = []\nsrc = [\".\", \"src\"]\ntarget-version = \"py310\"\nexclude = []\n\n[rules.DM002]\nmin-nullable-fields = 3\nmin-nullable-ratio = 0.3\n"
        );
    }
}
