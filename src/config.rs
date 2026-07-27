use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::fs;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;

use globset::{GlobBuilder, GlobMatcher};
use serde::Deserialize;

use crate::rules::{RULES, RuleCode};

const DEFAULT_MIN_NULLABLE_FIELDS: usize = 3;
const DEFAULT_MIN_NULLABLE_RATIO: f64 = 0.30;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TargetVersion {
    #[default]
    Py310,
    Py311,
    Py312,
    Py313,
    Py314,
    Py315,
}

impl Display for TargetVersion {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Py310 => "py310",
            Self::Py311 => "py311",
            Self::Py312 => "py312",
            Self::Py313 => "py313",
            Self::Py314 => "py314",
            Self::Py315 => "py315",
        })
    }
}

impl FromStr for TargetVersion {
    type Err = ConfigError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "py310" => Ok(Self::Py310),
            "py311" => Ok(Self::Py311),
            "py312" => Ok(Self::Py312),
            "py313" => Ok(Self::Py313),
            "py314" => Ok(Self::Py314),
            "py315" => Ok(Self::Py315),
            _ => Err(ConfigError::new(format!(
                "invalid target-version `{value}`; expected py310, py311, py312, py313, py314, or py315"
            ))),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Dm002Config {
    pub min_nullable_fields: usize,
    pub min_nullable_ratio: f64,
}

impl Default for Dm002Config {
    fn default() -> Self {
        Self {
            min_nullable_fields: DEFAULT_MIN_NULLABLE_FIELDS,
            min_nullable_ratio: DEFAULT_MIN_NULLABLE_RATIO,
        }
    }
}

#[derive(Clone, Debug)]
pub struct PerFileIgnore {
    pub pattern: String,
    pub rules: Vec<RuleCode>,
    pub reason: String,
    matcher: GlobMatcher,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub select: Vec<String>,
    pub ignore: Vec<String>,
    pub enabled_rules: Vec<RuleCode>,
    pub src: Vec<PathBuf>,
    pub target_version: TargetVersion,
    pub exclude: Vec<String>,
    pub dm002: Dm002Config,
    pub per_file_ignores: Vec<PerFileIgnore>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            select: Vec::new(),
            ignore: Vec::new(),
            enabled_rules: Vec::new(),
            src: vec![PathBuf::from("."), PathBuf::from("src")],
            target_version: TargetVersion::default(),
            exclude: Vec::new(),
            dm002: Dm002Config::default(),
            per_file_ignores: Vec::new(),
        }
    }
}

impl Config {
    pub fn write(&self, output: &mut dyn Write) -> io::Result<()> {
        writeln!(output, "select = {}", format_strings(&self.select))?;
        writeln!(output, "ignore = {}", format_strings(&self.ignore))?;
        let src = self
            .src
            .iter()
            .map(|path| normalize_path(path))
            .collect::<Vec<_>>();
        writeln!(output, "src = {}", format_strings(&src))?;
        writeln!(output, "target-version = \"{}\"", self.target_version)?;
        writeln!(output, "exclude = {}", format_strings(&self.exclude))
    }

    fn from_raw(raw: RawConfig) -> Result<Self, ConfigError> {
        let select = raw.select.unwrap_or_default();
        let ignore = raw.ignore.unwrap_or_default();
        validate_selectors("select", &select)?;
        validate_selectors("ignore", &ignore)?;

        let enabled_rules = RULES
            .iter()
            .filter(|rule| {
                select
                    .iter()
                    .any(|selector| selector_matches(selector, rule.code))
                    && !ignore
                        .iter()
                        .any(|selector| selector_matches(selector, rule.code))
            })
            .map(|rule| RuleCode::from_str(rule.code).expect("registry rule codes are valid"))
            .collect();

        let src = raw
            .src
            .unwrap_or_else(|| vec![".".to_owned(), "src".to_owned()]);
        if src.is_empty() {
            return Err(ConfigError::new("src must contain at least one path"));
        }
        let src = src
            .into_iter()
            .map(|path| {
                validate_relative_path("src", &path)?;
                Ok(PathBuf::from(path))
            })
            .collect::<Result<Vec<_>, ConfigError>>()?;

        let target_version = raw.target_version.as_deref().unwrap_or("py310").parse()?;
        let exclude = raw.exclude.unwrap_or_default();
        for pattern in &exclude {
            validate_pattern("exclude", pattern)?;
        }

        let dm002 = raw
            .rules
            .and_then(|rules| rules.dm002)
            .map_or_else(|| Ok(Dm002Config::default()), Dm002Config::from_raw)?;
        let per_file_ignores = raw
            .per_file_ignores
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .map(|(index, entry)| PerFileIgnore::from_raw(index, entry))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            select,
            ignore,
            enabled_rules,
            src,
            target_version,
            exclude,
            dm002,
            per_file_ignores,
        })
    }
}

impl Dm002Config {
    fn from_raw(raw: RawDm002Config) -> Result<Self, ConfigError> {
        let min_nullable_fields = raw
            .min_nullable_fields
            .unwrap_or(DEFAULT_MIN_NULLABLE_FIELDS);
        if min_nullable_fields < 1 {
            return Err(ConfigError::new(
                "rules.DM002.min-nullable-fields must be at least 1",
            ));
        }

        let min_nullable_ratio = raw.min_nullable_ratio.unwrap_or(DEFAULT_MIN_NULLABLE_RATIO);
        if !min_nullable_ratio.is_finite() || min_nullable_ratio <= 0.0 || min_nullable_ratio > 1.0
        {
            return Err(ConfigError::new(
                "rules.DM002.min-nullable-ratio must be greater than 0.0 and no greater than 1.0",
            ));
        }

        Ok(Self {
            min_nullable_fields,
            min_nullable_ratio,
        })
    }
}

impl PerFileIgnore {
    pub fn matches(&self, relative_path: &Path) -> bool {
        self.matcher.is_match(normalize_path(relative_path))
    }

    fn from_raw(index: usize, raw: RawPerFileIgnore) -> Result<Self, ConfigError> {
        let location = format!("per-file-ignores[{}]", index + 1);
        if raw.pattern.trim().is_empty() {
            return Err(ConfigError::new(format!(
                "{location}.pattern must not be empty"
            )));
        }
        validate_pattern(&format!("{location}.pattern"), &raw.pattern)?;
        if raw.rules.is_empty() {
            return Err(ConfigError::new(format!(
                "{location}.rules must contain at least one full rule code"
            )));
        }
        let rules = raw
            .rules
            .into_iter()
            .map(|value| {
                let code = RuleCode::from_str(&value).map_err(|_| {
                    ConfigError::new(format!(
                        "{location}.rules contains `{value}`; expected a full rule code"
                    ))
                })?;
                if code.as_str() == "SUP001" {
                    return Err(ConfigError::new(format!(
                        "{location}.rules cannot contain SUP001"
                    )));
                }
                Ok(code)
            })
            .collect::<Result<Vec<_>, ConfigError>>()?;
        if raw.reason.trim().is_empty() {
            return Err(ConfigError::new(format!(
                "{location}.reason must not be empty"
            )));
        }

        Ok(Self {
            matcher: build_glob(&raw.pattern)?.compile_matcher(),
            pattern: raw.pattern,
            rules,
            reason: raw.reason,
        })
    }
}

#[derive(Clone, Debug)]
pub struct UvWorkspace {
    pub members: Vec<String>,
    pub exclude: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Pyproject {
    pub config: Option<Config>,
    pub workspace: Option<UvWorkspace>,
}

impl Pyproject {
    pub fn read(path: &Path) -> Result<Self, ConfigError> {
        let source = fs::read_to_string(path).map_err(|error| {
            ConfigError::new(format!("failed to read {}: {error}", path.display()))
        })?;
        let raw: RawPyproject = toml::from_str(&source).map_err(|error| {
            ConfigError::new(format!("failed to parse {}: {error}", path.display()))
        })?;
        let config = raw
            .tool
            .ddd_lint
            .map(Config::from_raw)
            .transpose()
            .map_err(|error| error.at(path))?;
        let workspace = raw
            .tool
            .uv
            .and_then(|uv| uv.workspace)
            .map(|workspace| UvWorkspace {
                members: workspace.members.unwrap_or_default(),
                exclude: workspace.exclude.unwrap_or_default(),
            });

        if let Some(workspace) = &workspace {
            for pattern in &workspace.members {
                validate_pattern("tool.uv.workspace.members", pattern)
                    .map_err(|error| error.at(path))?;
            }
            for pattern in &workspace.exclude {
                validate_pattern("tool.uv.workspace.exclude", pattern)
                    .map_err(|error| error.at(path))?;
            }
        }

        Ok(Self { config, workspace })
    }
}

#[derive(Debug, Deserialize, Default)]
struct RawPyproject {
    #[serde(default)]
    tool: RawTool,
}

#[derive(Debug, Deserialize, Default)]
struct RawTool {
    #[serde(default, rename = "ddd-lint")]
    ddd_lint: Option<RawConfig>,
    #[serde(default)]
    uv: Option<RawUv>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
struct RawConfig {
    select: Option<Vec<String>>,
    ignore: Option<Vec<String>>,
    src: Option<Vec<String>>,
    target_version: Option<String>,
    exclude: Option<Vec<String>>,
    rules: Option<RawRules>,
    per_file_ignores: Option<Vec<RawPerFileIgnore>>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct RawRules {
    #[serde(rename = "DM002")]
    dm002: Option<RawDm002Config>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
struct RawDm002Config {
    min_nullable_fields: Option<usize>,
    min_nullable_ratio: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPerFileIgnore {
    pattern: String,
    rules: Vec<String>,
    reason: String,
}

#[derive(Debug, Deserialize, Default)]
struct RawUv {
    workspace: Option<RawUvWorkspace>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct RawUvWorkspace {
    members: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigError(String);

impl ConfigError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    fn at(self, path: &Path) -> Self {
        Self(format!(
            "invalid configuration in {}: {}",
            path.display(),
            self.0
        ))
    }
}

impl Display for ConfigError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for ConfigError {}

pub fn compile_globs(patterns: &[String]) -> Result<globset::GlobSet, ConfigError> {
    let mut builder = globset::GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(build_glob(pattern)?);
    }
    builder
        .build()
        .map_err(|error| ConfigError::new(format!("invalid glob set: {error}")))
}

fn validate_selectors(field: &str, selectors: &[String]) -> Result<(), ConfigError> {
    for selector in selectors {
        if !RULES
            .iter()
            .any(|rule| selector_matches(selector, rule.code))
        {
            return Err(ConfigError::new(format!(
                "unknown rule selector `{selector}` in {field}"
            )));
        }
    }
    Ok(())
}

fn selector_matches(selector: &str, code: &str) -> bool {
    selector == "ALL"
        || selector == code
        || (selector == rule_family(code) && code.starts_with(selector))
}

fn rule_family(code: &str) -> &str {
    code.trim_end_matches(|character: char| character.is_ascii_digit())
}

fn validate_relative_path(field: &str, value: &str) -> Result<(), ConfigError> {
    let path = Path::new(value);
    if value.trim().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
    {
        return Err(ConfigError::new(format!(
            "{field} path `{value}` must be relative to the configuration root"
        )));
    }
    Ok(())
}

fn validate_pattern(field: &str, pattern: &str) -> Result<(), ConfigError> {
    validate_relative_path(field, pattern)?;
    build_glob(pattern).map(|_| ())
}

fn build_glob(pattern: &str) -> Result<globset::Glob, ConfigError> {
    GlobBuilder::new(pattern)
        .literal_separator(true)
        .backslash_escape(false)
        .build()
        .map_err(|error| ConfigError::new(format!("invalid glob `{pattern}`: {error}")))
}

pub fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn format_strings(values: &[String]) -> String {
    let values = values
        .iter()
        .map(|value| format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\"")))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{values}]")
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use tempfile::tempdir;

    use super::{Config, Pyproject, RawConfig, TargetVersion, normalize_path};

    fn parse_config(source: &str) -> Result<Config, String> {
        let raw: RawConfig = toml::from_str(source).map_err(|error| error.to_string())?;
        Config::from_raw(raw).map_err(|error| error.to_string())
    }

    #[test]
    fn defaults_enable_no_rules() {
        let config = parse_config("").expect("defaults should resolve");

        assert!(config.enabled_rules.is_empty());
        assert_eq!(config.src, [PathBuf::from("."), PathBuf::from("src")]);
        assert_eq!(config.target_version, TargetVersion::Py310);
        assert_eq!(config.dm002.min_nullable_fields, 3);
        assert_eq!(config.dm002.min_nullable_ratio, 0.30);
    }

    #[test]
    fn selectors_expand_and_ignore_wins() {
        let config = parse_config("select = [\"ALL\"]\nignore = [\"CT\", \"DM002\"]\n")
            .expect("selectors should resolve");
        let enabled = config
            .enabled_rules
            .iter()
            .map(|code| code.as_str())
            .collect::<Vec<_>>();

        assert_eq!(enabled, ["DM001", "DM003", "DI001", "SUP001"]);
    }

    #[test]
    fn rejects_unknown_selectors() {
        let error = parse_config("select = [\"D\"]\n").expect_err("partial family should fail");

        assert!(error.contains("unknown rule selector `D`"), "{error}");
    }

    #[test]
    fn validates_every_target_version() {
        for target in ["py310", "py311", "py312", "py313", "py314", "py315"] {
            parse_config(&format!("target-version = \"{target}\"\n"))
                .unwrap_or_else(|error| panic!("{target}: {error}"));
        }
        let error = parse_config("target-version = \"py39\"\n")
            .expect_err("unsupported target should fail");
        assert!(error.contains("invalid target-version `py39`"), "{error}");
    }

    #[test]
    fn validates_dm002_threshold_boundaries() {
        parse_config("[rules.DM002]\nmin-nullable-fields = 1\nmin-nullable-ratio = 1.0\n")
            .expect("inclusive boundaries should pass");
        for source in [
            "[rules.DM002]\nmin-nullable-fields = 0\n",
            "[rules.DM002]\nmin-nullable-ratio = 0.0\n",
            "[rules.DM002]\nmin-nullable-ratio = 1.01\n",
        ] {
            parse_config(source).expect_err("invalid threshold should fail");
        }
    }

    #[test]
    fn validates_structured_per_file_ignores() {
        let config = parse_config(
            "[[per-file-ignores]]\npattern = \"tests/fixtures/**\"\nrules = [\"DM001\", \"CT003\"]\nreason = \"Intentional malformed examples\"\n",
        )
        .expect("structured entry should pass");
        assert_eq!(config.per_file_ignores.len(), 1);
        assert!(
            config.per_file_ignores[0].matches(std::path::Path::new("tests\\fixtures\\invalid.py"))
        );
        assert!(!config.per_file_ignores[0].matches(std::path::Path::new("src/model.py")));

        for source in [
            "[[per-file-ignores]]\npattern = \"tests/**\"\nrules = [\"DM\"]\nreason = \"Needed\"\n",
            "[[per-file-ignores]]\npattern = \"tests/**\"\nrules = [\"ALL\"]\nreason = \"Needed\"\n",
            "[[per-file-ignores]]\npattern = \"tests/**\"\nrules = [\"SUP001\"]\nreason = \"Needed\"\n",
            "[[per-file-ignores]]\npattern = \"tests/**\"\nrules = [\"DM001\"]\nreason = \"  \"\n",
        ] {
            parse_config(source).expect_err("invalid per-file ignore should fail");
        }
    }

    #[test]
    fn rejects_invalid_globs_and_normalizes_separators() {
        let error = parse_config("exclude = [\"[\"]\n").expect_err("invalid glob should fail");
        assert!(error.contains("invalid glob `[`"), "{error}");
        assert_eq!(
            normalize_path(std::path::Path::new("src\\domain\\model.py")),
            "src/domain/model.py"
        );
    }

    #[test]
    fn malformed_pyproject_error_names_file_and_location() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("pyproject.toml");
        fs::write(&path, "[tool.ddd-lint\nselect = []\n").expect("fixture should write");

        let error = Pyproject::read(&path)
            .expect_err("malformed TOML should fail")
            .to_string();

        assert!(error.contains(&path.display().to_string()), "{error}");
        assert!(error.contains("failed to parse"), "{error}");
    }
}
