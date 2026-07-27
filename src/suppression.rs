use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::config::PerFileIgnore;
use crate::diagnostics::{Diagnostic, DiagnosticSet, SourceError};
use crate::graph::GraphFile;
use crate::python::{ByteRange, SourceFile, SourceKind, SyntaxKind, SyntaxNode};
use crate::rules::RuleCode;

const PREFIX: &str = "# ddd-lint: ";

#[derive(Clone, Debug)]
pub struct DesignExceptions {
    source: Vec<SourceException>,
    configuration: Vec<PerFileIgnore>,
}

impl DesignExceptions {
    pub fn parse(files: &[GraphFile], configuration: &[PerFileIgnore]) -> (Self, Vec<SourceError>) {
        let mut source = Vec::new();
        let mut errors = Vec::new();
        for file in files {
            if !file.diagnostic || file.source_kind == SourceKind::Stub {
                continue;
            }
            let Some(source_file) = &file.source else {
                continue;
            };
            parse_file(file, source_file, &mut source, &mut errors);
        }
        errors.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then_with(|| left.range.start().cmp(&right.range.start()))
        });
        (
            Self {
                source,
                configuration: configuration.to_vec(),
            },
            errors,
        )
    }

    #[cfg(test)]
    pub fn apply(&self, diagnostics: Vec<Diagnostic>, root: &Path) -> DiagnosticSet {
        self.apply_with_hygiene(diagnostics, root, &[], &[], None)
    }

    pub fn apply_with_hygiene(
        &self,
        diagnostics: Vec<Diagnostic>,
        root: &Path,
        enabled: &[RuleCode],
        analyzed_files: &[PathBuf],
        config_path: Option<&Path>,
    ) -> DiagnosticSet {
        let mut remaining = diagnostics
            .iter()
            .filter(|diagnostic| !self.suppresses(diagnostic, root))
            .cloned()
            .collect::<Vec<_>>();
        if enabled.iter().any(|rule| rule.as_str() == "SUP001") {
            remaining.extend(self.unused_diagnostics(
                &diagnostics,
                root,
                enabled,
                analyzed_files,
                config_path,
            ));
        }
        DiagnosticSet::new(remaining)
    }

    fn suppresses(&self, diagnostic: &Diagnostic, root: &Path) -> bool {
        let code = diagnostic.rule.as_str();
        let relative = diagnostic
            .path
            .strip_prefix(root)
            .unwrap_or(&diagnostic.path);
        if self.configuration.iter().any(|entry| {
            entry.matches(relative) && entry.rules.iter().any(|rule| rule.as_str() == code)
        }) {
            return true;
        }
        self.source.iter().any(|exception| {
            exception.path == diagnostic.path
                && exception.codes.iter().any(|rule| rule.as_str() == code)
                && exception.scope.contains(diagnostic.range.start())
        })
    }

    fn unused_diagnostics(
        &self,
        diagnostics: &[Diagnostic],
        root: &Path,
        enabled: &[RuleCode],
        analyzed_files: &[PathBuf],
        config_path: Option<&Path>,
    ) -> Vec<Diagnostic> {
        let enabled = enabled
            .iter()
            .map(|rule| rule.as_str())
            .collect::<BTreeSet<_>>();
        let sup = RuleCode::from_str("SUP001").expect("SUP001 is registered");
        let mut unused = Vec::new();
        for exception in &self.source {
            for code in &exception.codes {
                let used = enabled.contains(code.as_str())
                    && diagnostics.iter().any(|diagnostic| {
                        diagnostic.path == exception.path
                            && diagnostic.rule == *code
                            && exception.scope.contains(diagnostic.range.start())
                    });
                if !used {
                    unused.push(Diagnostic::new(
                        &exception.path,
                        exception.range,
                        sup,
                        format!(
                            "source design exception for {} suppresses no enabled diagnostic",
                            code.as_str()
                        ),
                        "remove the unused rule code or enable the diagnostic it documents",
                    ));
                }
            }
        }
        let Some(config_path) = config_path else {
            return unused;
        };
        for entry in &self.configuration {
            let matching_files = analyzed_files
                .iter()
                .filter(|path| entry.matches(path.strip_prefix(root).unwrap_or(path)))
                .collect::<Vec<_>>();
            for (index, code) in entry.rules.iter().enumerate() {
                let used = !matching_files.is_empty()
                    && enabled.contains(code.as_str())
                    && diagnostics.iter().any(|diagnostic| {
                        diagnostic.rule == *code
                            && entry.matches(
                                diagnostic
                                    .path
                                    .strip_prefix(root)
                                    .unwrap_or(&diagnostic.path),
                            )
                    });
                if !used {
                    let no_files = matching_files.is_empty();
                    unused.push(Diagnostic::new(
                        config_path,
                        entry
                            .rule_ranges
                            .get(index)
                            .copied()
                            .unwrap_or(entry.pattern_range),
                        sup,
                        if no_files {
                            format!(
                                "per-file design exception for {} has pattern `{}` matching no analyzed file",
                                code.as_str(),
                                entry.pattern
                            )
                        } else {
                            format!(
                                "per-file design exception for {} suppresses no enabled diagnostic",
                                code.as_str()
                            )
                        },
                        "remove the unused rule code or narrow the exception to an active diagnostic",
                    ));
                }
            }
        }
        unused
    }
}

#[derive(Clone, Debug)]
struct SourceException {
    path: PathBuf,
    codes: Vec<RuleCode>,
    #[allow(dead_code)]
    reason: String,
    range: ByteRange,
    scope: Scope,
}

#[derive(Clone, Copy, Debug)]
enum Scope {
    File,
    Range(ByteRange),
}

impl Scope {
    fn contains(self, offset: usize) -> bool {
        match self {
            Self::File => true,
            Self::Range(range) => range.start() <= offset && offset < range.end(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DirectiveKind {
    Ignore,
    FileIgnore,
}

struct ParsedDirective {
    kind: DirectiveKind,
    codes: Vec<RuleCode>,
    reason: String,
}

fn parse_file(
    file: &GraphFile,
    source_file: &SourceFile,
    exceptions: &mut Vec<SourceException>,
    errors: &mut Vec<SourceError>,
) {
    let first_statement = statements(&file.nodes).first().copied();
    for comment in &file.comments {
        if !comment.text.starts_with(PREFIX) {
            continue;
        }
        let parsed = match parse_directive(&comment.text) {
            Ok(parsed) => parsed,
            Err(message) => {
                errors.push(SourceError::new(&file.path, comment.range, message));
                continue;
            }
        };
        let own_line = is_own_line(source_file.text(), comment.range.start());
        let scope = match parsed.kind {
            DirectiveKind::FileIgnore => {
                if !own_line
                    || first_statement
                        .is_some_and(|statement| comment.range.start() >= statement.range.start())
                    || !valid_file_preamble(source_file.text(), comment.range.start())
                {
                    errors.push(SourceError::new(
                        &file.path,
                        comment.range,
                        "file-ignore must be an own-line directive before the first statement; only a shebang, encoding comment, or stacked DDD Lint directive may precede it",
                    ));
                    continue;
                }
                Scope::File
            }
            DirectiveKind::Ignore if own_line => {
                let Some(statement) = next_statement(&file.nodes, comment.range.end()) else {
                    errors.push(SourceError::new(
                        &file.path,
                        comment.range,
                        "own-line ignore must immediately precede a logical statement",
                    ));
                    continue;
                };
                if !valid_attachment(source_file, comment.range, statement.range) {
                    errors.push(SourceError::new(
                        &file.path,
                        comment.range,
                        "own-line ignore must immediately precede a logical statement without a blank line or unrelated comment",
                    ));
                    continue;
                }
                Scope::Range(statement.range)
            }
            DirectiveKind::Ignore => Scope::Range(physical_line_range(
                source_file.text(),
                comment.range.start(),
            )),
        };
        exceptions.push(SourceException {
            path: file.path.clone(),
            codes: parsed.codes,
            reason: parsed.reason,
            range: comment.range,
            scope,
        });
    }
}

fn parse_directive(comment: &str) -> Result<ParsedDirective, String> {
    let body = comment
        .strip_prefix(PREFIX)
        .expect("caller checked directive prefix");
    let (kind, rest) = if let Some(rest) = body.strip_prefix("ignore[") {
        (DirectiveKind::Ignore, rest)
    } else if let Some(rest) = body.strip_prefix("file-ignore[") {
        (DirectiveKind::FileIgnore, rest)
    } else {
        return Err(
            "invalid ddd-lint directive; expected ignore[FULL_CODE] or file-ignore[FULL_CODE]"
                .to_owned(),
        );
    };
    let Some((codes, suffix)) = rest.split_once(']') else {
        return Err("invalid ddd-lint directive; missing closing `]`".to_owned());
    };
    if codes.trim().is_empty() {
        return Err("ddd-lint directive must contain at least one full rule code".to_owned());
    }
    if !suffix.starts_with(" - ") {
        return Err("ddd-lint directive requires the exact ` - ` reason delimiter".to_owned());
    }
    let reason = &suffix[3..];
    if reason.trim().is_empty() {
        return Err("ddd-lint directive reason must not be empty".to_owned());
    }
    let codes = codes
        .split(',')
        .map(str::trim)
        .map(|value| {
            if value == "SUP001" {
                return Err("ddd-lint directives cannot suppress SUP001".to_owned());
            }
            RuleCode::from_str(value).map_err(|_| {
                format!("invalid local rule code `{value}`; expected an uppercase full rule code")
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ParsedDirective {
        kind,
        codes,
        reason: reason.trim().to_owned(),
    })
}

fn statements(nodes: &[SyntaxNode]) -> Vec<SyntaxNode> {
    let mut statements = nodes
        .iter()
        .copied()
        .filter(|node| is_statement(node.kind))
        .collect::<Vec<_>>();
    statements.sort_by_key(|node| (node.range.start(), node.depth));
    statements.dedup_by_key(|node| (node.range.start(), node.range.end(), node.depth));
    statements
}

fn next_statement(nodes: &[SyntaxNode], offset: usize) -> Option<SyntaxNode> {
    statements(nodes)
        .into_iter()
        .filter(|node| node.range.start() >= offset)
        .min_by_key(|node| (node.range.start(), node.depth))
}

fn is_statement(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::FunctionDefinition
            | SyntaxKind::ClassDefinition
            | SyntaxKind::ReturnStatement
            | SyntaxKind::TypeAliasStatement
            | SyntaxKind::Assignment
            | SyntaxKind::AnnotatedAssignment
            | SyntaxKind::IfStatement
            | SyntaxKind::MatchStatement
            | SyntaxKind::ImportStatement
            | SyntaxKind::ImportFromStatement
            | SyntaxKind::ExpressionStatement
            | SyntaxKind::OtherStatement
    )
}

fn is_own_line(source: &str, offset: usize) -> bool {
    source[line_start(source, offset)..offset].trim().is_empty()
}

fn physical_line_range(source: &str, offset: usize) -> ByteRange {
    ByteRange::new(line_start(source, offset), line_end(source, offset))
}

fn line_start(source: &str, offset: usize) -> usize {
    source[..offset].rfind('\n').map_or(0, |index| index + 1)
}

fn line_end(source: &str, offset: usize) -> usize {
    source[offset..]
        .find('\n')
        .map_or(source.len(), |index| offset + index)
}

fn line_number(source: &str, offset: usize) -> usize {
    source[..offset]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1
}

fn valid_attachment(source: &SourceFile, comment: ByteRange, statement: ByteRange) -> bool {
    let comment_line = line_number(source.text(), comment.start());
    let statement_line = line_number(source.text(), statement.start());
    if statement_line <= comment_line {
        return false;
    }
    source
        .text()
        .lines()
        .enumerate()
        .skip(comment_line)
        .take(statement_line.saturating_sub(comment_line + 1))
        .all(|(_, line)| line.trim_start().starts_with(PREFIX))
}

fn valid_file_preamble(source: &str, directive_offset: usize) -> bool {
    let directive_line = line_number(source, directive_offset);
    source
        .lines()
        .take(directive_line - 1)
        .enumerate()
        .all(|(index, line)| {
            let trimmed = line.trim();
            (index == 0 && trimmed.starts_with("#!"))
                || (index <= 1 && is_encoding_comment(trimmed))
                || trimmed.starts_with(PREFIX)
        })
}

fn is_encoding_comment(line: &str) -> bool {
    if !line.starts_with('#') {
        return false;
    }
    let Some(marker) = line
        .find("coding:")
        .map(|index| index + "coding:".len())
        .or_else(|| line.find("coding=").map(|index| index + "coding=".len()))
    else {
        return false;
    };
    line[marker..]
        .trim_start()
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::str::FromStr;

    use crate::diagnostics::Diagnostic;
    use crate::graph::GraphFile;
    use crate::project::Project;
    use crate::python::{ByteRange, PythonVersion, SourceKind, parse_module};
    use crate::rules::RuleCode;

    use super::DesignExceptions;

    fn graph_file(path: &str, source: &str) -> GraphFile {
        let parsed = parse_module(path, source, PythonVersion::Py310).expect("valid Python");
        GraphFile {
            path: path.into(),
            module: Some("module".to_owned()),
            source_kind: SourceKind::from_path(Path::new(path)),
            diagnostic: true,
            source: Some(parsed.source().clone()),
            comments: parsed.comments().to_vec(),
            nodes: parsed.nodes().to_vec(),
        }
    }

    fn diagnostic(path: &str, source: &str, needle: &str, code: &str) -> Diagnostic {
        let start = source.find(needle).expect("diagnostic needle");
        Diagnostic::new(
            path,
            ByteRange::new(start, start + needle.len()),
            RuleCode::from_str(code).expect("known rule"),
            "observed",
            "remediate",
        )
    }

    #[test]
    fn trailing_directive_covers_only_physical_line_and_exact_code() {
        let source = "first = 1  # ddd-lint: ignore[DM001] - accepted\nsecond = 2\n";
        let file = graph_file("model.py", source);
        let (exceptions, errors) = DesignExceptions::parse(&[file], &[]);
        assert!(errors.is_empty(), "{errors:?}");

        let remaining = exceptions.apply(
            vec![
                diagnostic("model.py", source, "first", "DM001"),
                diagnostic("model.py", source, "first", "DM003"),
                diagnostic("model.py", source, "second", "DM001"),
            ],
            Path::new("."),
        );
        assert_eq!(remaining.as_slice().len(), 2);
        assert_eq!(remaining.as_slice()[0].rule.as_str(), "DM003");
        assert_eq!(
            remaining.as_slice()[1].range.start(),
            source.find("second").unwrap()
        );
    }

    #[test]
    fn own_line_directives_cover_statements_declarations_and_multiline_ranges() {
        let source = "# ddd-lint: ignore[DM001] - multiline\nvalue = (\n    1\n    + 2\n)\n# ddd-lint: ignore[CT003] - adapter\ndef dispatch():\n    branch = 1\n    return branch\n# ddd-lint: ignore[DM003] - aggregate declaration\nclass Aggregate:\n    field = 1\noutside = 2\n";
        let file = graph_file("model.py", source);
        let (exceptions, errors) = DesignExceptions::parse(&[file], &[]);
        assert!(errors.is_empty(), "{errors:?}");
        let remaining = exceptions.apply(
            vec![
                diagnostic("model.py", source, "+ 2", "DM001"),
                diagnostic("model.py", source, "branch =", "CT003"),
                diagnostic("model.py", source, "field =", "DM003"),
                diagnostic("model.py", source, "outside", "CT003"),
            ],
            Path::new("."),
        );
        assert_eq!(remaining.as_slice().len(), 1);
        assert_eq!(
            remaining.as_slice()[0].range.start(),
            source.find("outside").unwrap()
        );
    }

    #[test]
    fn directive_inside_multiline_statement_covers_only_its_line() {
        let source = "value = (\n    1  # ddd-lint: ignore[DM001] - first operand\n    + 2\n)\n";
        let file = graph_file("model.py", source);
        let (exceptions, errors) = DesignExceptions::parse(&[file], &[]);
        assert!(errors.is_empty(), "{errors:?}");
        let remaining = exceptions.apply(
            vec![
                diagnostic("model.py", source, "1  #", "DM001"),
                diagnostic("model.py", source, "+ 2", "DM001"),
            ],
            Path::new("."),
        );
        assert_eq!(remaining.as_slice().len(), 1);
        assert_eq!(
            remaining.as_slice()[0].range.start(),
            source.find("+ 2").unwrap()
        );
    }

    #[test]
    fn stacked_directives_attach_before_first_decorator() {
        let source = "# ddd-lint: ignore[DM001] - first\n# ddd-lint: ignore[DM003] - second\n@decorator\ndef build():\n    value = 1\n";
        let file = graph_file("model.py", source);
        let (exceptions, errors) = DesignExceptions::parse(&[file], &[]);
        assert!(errors.is_empty(), "{errors:?}");
        let remaining = exceptions.apply(
            vec![
                diagnostic("model.py", source, "value", "DM001"),
                diagnostic("model.py", source, "value", "DM003"),
            ],
            Path::new("."),
        );
        assert!(remaining.as_slice().is_empty());
    }

    #[test]
    fn file_directives_allow_shebang_encoding_and_stacking() {
        let source = "#!/usr/bin/env python\n# coding: utf-8\n# ddd-lint: file-ignore[DM001] - generated\n# ddd-lint: file-ignore[DM003] - generated\nvalue = 1\n";
        let file = graph_file("model.py", source);
        let (exceptions, errors) = DesignExceptions::parse(&[file], &[]);
        assert!(errors.is_empty(), "{errors:?}");
        let remaining = exceptions.apply(
            vec![
                diagnostic("model.py", source, "value", "DM001"),
                diagnostic("model.py", source, "value", "DM003"),
            ],
            Path::new("."),
        );
        assert!(remaining.as_slice().is_empty());
    }

    #[test]
    fn rejects_attachment_gaps_and_invalid_file_preambles() {
        for source in [
            "# ddd-lint: ignore[DM001] - reason\n\nvalue = 1\n",
            "# ddd-lint: ignore[DM001] - reason\n# unrelated\nvalue = 1\n",
            "# unrelated\n# ddd-lint: file-ignore[DM001] - reason\nvalue = 1\n",
            "# coding:\n# ddd-lint: file-ignore[DM001] - reason\nvalue = 1\n",
            "\n# ddd-lint: file-ignore[DM001] - reason\nvalue = 1\n",
            "value = 1\n# ddd-lint: file-ignore[DM001] - reason\n",
        ] {
            let file = graph_file("model.py", source);
            let (_, errors) = DesignExceptions::parse(&[file], &[]);
            assert_eq!(errors.len(), 1, "{source:?}: {errors:?}");
        }
    }

    #[test]
    fn rejects_malformed_unknown_blanket_and_sup001_directives() {
        for directive in [
            "# ddd-lint: ignore - reason",
            "# ddd-lint: ignore[] - reason",
            "# ddd-lint: ignore[DM001]- reason",
            "# ddd-lint: ignore[DM001] - ",
            "# ddd-lint: ignore[DM] - reason",
            "# ddd-lint: ignore[ALL] - reason",
            "# ddd-lint: ignore[dm001] - reason",
            "# ddd-lint: ignore[UNKNOWN] - reason",
            "# ddd-lint: ignore[SUP001] - reason",
        ] {
            let source = format!("{directive}\nvalue = 1\n");
            let file = graph_file("model.py", &source);
            let (_, errors) = DesignExceptions::parse(&[file], &[]);
            assert_eq!(errors.len(), 1, "{directive}: {errors:?}");
        }
    }

    #[test]
    fn ignores_all_directives_in_stubs() {
        let source = "# ddd-lint: ignore[UNKNOWN]\nvalue: int\n";
        let file = graph_file("model.pyi", source);
        let (_, errors) = DesignExceptions::parse(&[file], &[]);
        assert!(errors.is_empty());
    }

    #[test]
    fn structured_per_file_exceptions_match_globs_and_exact_rules() {
        let directory = tempfile::tempdir().expect("temporary directory");
        fs::write(
            directory.path().join("pyproject.toml"),
            "[tool.ddd-lint]\n[[tool.ddd-lint.per-file-ignores]]\npattern = \"tests/fixtures/**\"\nrules = [\"DM001\"]\nreason = \"Malformed external payloads\"\n",
        )
        .expect("configuration fixture");
        let project = Project::resolve(directory.path(), None).expect("project");
        let (exceptions, errors) = DesignExceptions::parse(&[], &project.config().per_file_ignores);
        assert!(errors.is_empty());
        let matched = project.root().join("tests/fixtures/model.py");
        let unmatched = project.root().join("src/model.py");
        let remaining = exceptions.apply(
            vec![
                Diagnostic::new(
                    &matched,
                    ByteRange::new(0, 1),
                    RuleCode::from_str("DM001").unwrap(),
                    "observed",
                    "remediate",
                ),
                Diagnostic::new(
                    &matched,
                    ByteRange::new(0, 1),
                    RuleCode::from_str("DM003").unwrap(),
                    "observed",
                    "remediate",
                ),
                Diagnostic::new(
                    &unmatched,
                    ByteRange::new(0, 1),
                    RuleCode::from_str("DM001").unwrap(),
                    "observed",
                    "remediate",
                ),
            ],
            project.root(),
        );
        assert_eq!(remaining.as_slice().len(), 2);
        assert_eq!(remaining.as_slice()[0].path, unmatched);
        assert_eq!(remaining.as_slice()[1].rule.as_str(), "DM003");
    }
}
