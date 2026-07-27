use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::config::normalize_path;
use crate::python::{ByteRange, ParseErrors, SourceFile, SourceLocation, SourceRange};
use crate::rules::RuleCode;

#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub path: PathBuf,
    pub range: ByteRange,
    pub rule: RuleCode,
    pub message: String,
    pub remediation: String,
}

impl Diagnostic {
    pub fn new(
        path: impl Into<PathBuf>,
        range: ByteRange,
        rule: RuleCode,
        message: impl Into<String>,
        remediation: impl Into<String>,
    ) -> Self {
        Self {
            path: path.into(),
            range,
            rule,
            message: message.into(),
            remediation: remediation.into(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct SourceError {
    pub path: PathBuf,
    pub range: ByteRange,
    pub message: String,
}

impl SourceError {
    pub fn new(path: impl Into<PathBuf>, range: ByteRange, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            range,
            message: message.into(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct DiagnosticSet {
    diagnostics: Vec<Diagnostic>,
}

impl DiagnosticSet {
    pub fn new(mut diagnostics: Vec<Diagnostic>) -> Self {
        diagnostics.sort_by(|left, right| {
            normalize_path(&left.path)
                .cmp(&normalize_path(&right.path))
                .then_with(|| left.range.start().cmp(&right.range.start()))
                .then_with(|| left.rule.as_str().cmp(right.rule.as_str()))
        });
        Self { diagnostics }
    }

    pub fn as_slice(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    pub fn write_human(
        &self,
        output: &mut dyn Write,
        root: &Path,
        sources: &SourceMap,
    ) -> io::Result<()> {
        for diagnostic in &self.diagnostics {
            let range = sources.range(&diagnostic.path, diagnostic.range);
            let rule = diagnostic.rule.summary();
            writeln!(
                output,
                "{}:{}:{}-{}:{}: {} {}: {}; help: {}; docs: {}",
                display_path(&diagnostic.path, root),
                range.start.line,
                range.start.column,
                range.end.line,
                range.end.column,
                rule.code,
                rule.name,
                diagnostic.message,
                diagnostic.remediation,
                rule.documentation
            )?;
        }
        Ok(())
    }

    pub fn write_json(
        &self,
        output: &mut dyn Write,
        root: &Path,
        sources: &SourceMap,
    ) -> io::Result<()> {
        let rendered = self
            .diagnostics
            .iter()
            .map(|diagnostic| {
                let rule = diagnostic.rule.summary();
                JsonDiagnostic {
                    code: rule.code,
                    name: rule.name,
                    family: rule.family.name,
                    path: display_path(&diagnostic.path, root),
                    range: sources.range(&diagnostic.path, diagnostic.range).into(),
                    message: &diagnostic.message,
                    remediation: &diagnostic.remediation,
                    documentation: rule.documentation,
                }
            })
            .collect::<Vec<_>>();
        serde_json::to_writer_pretty(&mut *output, &rendered).map_err(io::Error::other)?;
        writeln!(output)
    }
}

#[derive(Clone, Debug, Default)]
pub struct SourceMap(BTreeMap<PathBuf, SourceFile>);

impl SourceMap {
    pub fn insert(&mut self, source: SourceFile) {
        self.0.insert(source.path().to_owned(), source);
    }

    pub fn range(&self, path: &Path, range: ByteRange) -> SourceRange {
        self.0
            .get(path)
            .and_then(|source| source.source_range(range))
            .unwrap_or(SourceRange {
                start: SourceLocation { line: 1, column: 1 },
                end: SourceLocation { line: 1, column: 1 },
            })
    }
}

#[derive(Serialize)]
struct JsonDiagnostic<'a> {
    code: &'static str,
    name: &'static str,
    family: &'static str,
    path: String,
    range: JsonRange,
    message: &'a str,
    remediation: &'a str,
    documentation: &'static str,
}

#[derive(Serialize)]
struct JsonRange {
    start: JsonLocation,
    end: JsonLocation,
}

impl From<SourceRange> for JsonRange {
    fn from(range: SourceRange) -> Self {
        Self {
            start: range.start.into(),
            end: range.end.into(),
        }
    }
}

#[derive(Serialize)]
struct JsonLocation {
    line: usize,
    column: usize,
}

impl From<SourceLocation> for JsonLocation {
    fn from(location: SourceLocation) -> Self {
        Self {
            line: location.line,
            column: location.column,
        }
    }
}

pub fn display_path(path: &Path, root: &Path) -> String {
    normalize_path(path.strip_prefix(root).unwrap_or(path))
}

pub fn write_source_findings_human(
    output: &mut dyn Write,
    root: &Path,
    sources: &SourceMap,
    source_errors: &[SourceError],
    syntax_errors: &[ParseErrors],
) -> io::Result<()> {
    let mut findings = collect_source_findings(root, sources, source_errors, syntax_errors);
    findings.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.offset.cmp(&right.offset))
            .then_with(|| left.kind.cmp(right.kind))
    });
    for finding in findings {
        writeln!(
            output,
            "{}:{}:{}-{}:{}: {}: {}",
            finding.path,
            finding.range.start.line,
            finding.range.start.column,
            finding.range.end.line,
            finding.range.end.column,
            finding.kind,
            finding.message
        )?;
    }
    Ok(())
}

pub fn write_source_findings_json(
    output: &mut dyn Write,
    root: &Path,
    sources: &SourceMap,
    source_errors: &[SourceError],
    syntax_errors: &[ParseErrors],
) -> io::Result<()> {
    let mut findings = collect_source_findings(root, sources, source_errors, syntax_errors);
    findings.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.offset.cmp(&right.offset))
            .then_with(|| left.kind.cmp(right.kind))
    });
    serde_json::to_writer_pretty(&mut *output, &findings).map_err(io::Error::other)?;
    writeln!(output)
}

#[derive(Serialize)]
struct JsonSourceFinding {
    kind: &'static str,
    path: String,
    range: JsonRange,
    message: String,
    #[serde(skip)]
    offset: usize,
}

fn collect_source_findings(
    root: &Path,
    sources: &SourceMap,
    source_errors: &[SourceError],
    syntax_errors: &[ParseErrors],
) -> Vec<JsonSourceFinding> {
    let mut findings = source_errors
        .iter()
        .map(|error| JsonSourceFinding {
            kind: "invalid-source-directive",
            path: display_path(&error.path, root),
            range: sources.range(&error.path, error.range).into(),
            message: error.message.clone(),
            offset: error.range.start(),
        })
        .collect::<Vec<_>>();
    for errors in syntax_errors {
        findings.extend(errors.errors().iter().map(|error| {
            JsonSourceFinding {
                kind: "syntax-error",
                path: display_path(errors.source().path(), root),
                range: errors
                    .source()
                    .source_range(error.range)
                    .unwrap_or(SourceRange {
                        start: SourceLocation { line: 1, column: 1 },
                        end: SourceLocation { line: 1, column: 1 },
                    })
                    .into(),
                message: format!("{}: {}", error.message, error.detail),
                offset: error.range.start(),
            }
        }));
    }
    findings
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{Diagnostic, DiagnosticSet, SourceMap};
    use crate::python::{ByteRange, SourceFile};
    use crate::rules::RuleCode;

    fn diagnostic(path: &str, offset: usize, code: &str) -> Diagnostic {
        Diagnostic::new(
            path,
            ByteRange::new(offset, offset + 1),
            RuleCode::from_str(code).expect("known rule"),
            "observed condition",
            "model alternatives explicitly",
        )
    }

    #[test]
    fn orders_by_path_offset_then_code() {
        let diagnostics = DiagnosticSet::new(vec![
            diagnostic("b.py", 0, "DM001"),
            diagnostic("a.py", 4, "DM003"),
            diagnostic("a.py", 4, "DM001"),
            diagnostic("a.py", 2, "CT001"),
        ]);

        assert_eq!(
            diagnostics
                .as_slice()
                .iter()
                .map(|item| (
                    item.path.to_string_lossy().into_owned(),
                    item.range.start(),
                    item.rule.as_str()
                ))
                .collect::<Vec<_>>(),
            [
                ("a.py".to_owned(), 2, "CT001"),
                ("a.py".to_owned(), 4, "DM001"),
                ("a.py".to_owned(), 4, "DM003"),
                ("b.py".to_owned(), 0, "DM001"),
            ]
        );
    }

    #[test]
    fn renders_stable_human_and_json_ranges() {
        let source = SourceFile::new("model.py", "name: str | None\n");
        let mut sources = SourceMap::default();
        sources.insert(source);
        let diagnostics = DiagnosticSet::new(vec![diagnostic("model.py", 0, "DM001")]);
        let mut human = Vec::new();
        diagnostics
            .write_human(&mut human, std::path::Path::new("."), &sources)
            .expect("human output");
        assert_eq!(
            String::from_utf8(human).expect("UTF-8"),
            "model.py:1:1-1:2: DM001 nullable-model-field: observed condition; help: model alternatives explicitly; docs: https://github.com/mpecovnik/ddd-lint/blob/main/docs/rules/dm001-nullable-model-field.md\n"
        );

        let mut json = Vec::new();
        diagnostics
            .write_json(&mut json, std::path::Path::new("."), &sources)
            .expect("JSON output");
        assert_eq!(
            String::from_utf8(json).expect("UTF-8"),
            "[\n  {\n    \"code\": \"DM001\",\n    \"name\": \"nullable-model-field\",\n    \"family\": \"Domain model\",\n    \"path\": \"model.py\",\n    \"range\": {\n      \"start\": {\n        \"line\": 1,\n        \"column\": 1\n      },\n      \"end\": {\n        \"line\": 1,\n        \"column\": 2\n      }\n    },\n    \"message\": \"observed condition\",\n    \"remediation\": \"model alternatives explicitly\",\n    \"documentation\": \"https://github.com/mpecovnik/ddd-lint/blob/main/docs/rules/dm001-nullable-model-field.md\"\n  }\n]\n"
        );
    }
}
