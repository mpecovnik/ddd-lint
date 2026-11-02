use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn ddd_lint() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ddd-lint"))
}

fn run(arguments: &[&str]) -> Output {
    ddd_lint()
        .args(arguments)
        .output()
        .expect("ddd-lint should run")
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout should be UTF-8")
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("stderr should be UTF-8")
}

fn write(path: &Path, source: &str) {
    fs::create_dir_all(path.parent().expect("fixture path should have a parent"))
        .expect("fixture directory should be created");
    fs::write(path, source).expect("fixture should be written");
}

#[test]
fn top_level_help_succeeds() {
    let output = run(&["--help"]);

    assert!(output.status.success(), "{}", stderr(&output));
    let stdout = stdout(&output);
    assert!(stdout.contains("Usage: ddd-lint"));
    assert!(stdout.contains("check"));
    assert!(stdout.contains("rule"));
    assert!(stdout.contains("config"));
}

#[test]
fn every_subcommand_help_succeeds() {
    for command in ["check", "rule", "config"] {
        let output = run(&[command, "--help"]);
        assert!(output.status.success(), "{command}: {}", stderr(&output));
        assert!(stdout(&output).contains("Usage:"), "{command}");
    }
}

#[test]
fn check_defaults_to_current_directory_and_no_rules() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let output = ddd_lint()
        .current_dir(directory.path())
        .arg("check")
        .output()
        .expect("ddd-lint should run");

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[test]
fn missing_path_exits_two() {
    let missing = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("path-that-cannot-exist");
    let output = ddd_lint()
        .arg("check")
        .arg(missing)
        .output()
        .expect("ddd-lint should run");

    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("path does not exist"));
}

#[test]
fn rule_prints_stable_summary() {
    let output = run(&["rule", "DM001"]);

    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        stdout(&output),
        "DM001: nullable-model-field\nFamily: Domain model (DM)\nConfiguration: none\nDocumentation: https://github.com/mpecovnik/ddd-lint/blob/main/docs/rules/dm001-nullable-model-field.md\n"
    );
}

#[test]
fn unknown_rule_exits_two() {
    let output = run(&["rule", "UNKNOWN"]);

    assert_eq!(output.status.code(), Some(2));
    let stderr = stderr(&output);
    assert!(stderr.contains("invalid value 'UNKNOWN'"), "{stderr}");
    assert!(stderr.contains("unknown rule code: UNKNOWN"), "{stderr}");
}

#[test]
fn config_reports_zero_selected_rules() {
    let output = run(&["config"]);

    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        stdout(&output),
        "select = []\nignore = []\nenabled-rules = []\nsrc = [\".\", \"src\"]\ntarget-version = \"py310\"\nexclude = []\n\n[rules.DM002]\nmin-nullable-fields = 3\nmin-nullable-ratio = 0.3\n\n[rules.CT003]\nmode = \"dispatch\"\n"
    );
}

#[test]
fn explicit_config_overrides_ancestor_discovery() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let nested = directory.path().join("nested");
    fs::create_dir_all(&nested).expect("nested directory");
    write(
        &directory.path().join("pyproject.toml"),
        "[tool.ddd-lint]\nselect = [\"DM001\"]\n",
    );
    let explicit = directory.path().join("policy.toml");
    write(
        &explicit,
        "[tool.ddd-lint]\nselect = [\"DI001\"]\ntarget-version = \"py315\"\n",
    );

    let output = ddd_lint()
        .current_dir(nested)
        .arg("config")
        .arg("--config")
        .arg(explicit)
        .output()
        .expect("ddd-lint should run");

    assert!(output.status.success(), "{}", stderr(&output));
    let output = stdout(&output);
    assert!(output.contains("select = [\"DI001\"]"), "{output}");
    assert!(output.contains("target-version = \"py315\""), "{output}");
}

#[test]
fn malformed_explicit_config_exits_two_with_context() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let config = directory.path().join("pyproject.toml");
    write(&config, "[tool.ddd-lint\nselect = []\n");

    let output = ddd_lint()
        .current_dir(directory.path())
        .arg("--config")
        .arg(&config)
        .arg("check")
        .output()
        .expect("ddd-lint should run");

    assert_eq!(output.status.code(), Some(2));
    let error = stderr(&output);
    assert!(error.contains("failed to parse"), "{error}");
    assert!(error.contains("pyproject.toml"), "{error}");
}

#[test]
fn syntax_error_exits_one_even_with_no_selected_rules() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write(
        &directory.path().join("pyproject.toml"),
        "[tool.ddd-lint]\n",
    );
    write(&directory.path().join("broken.py"), "def broken(:\n");

    let output = ddd_lint()
        .current_dir(directory.path())
        .arg("check")
        .output()
        .expect("ddd-lint should run");

    assert_eq!(output.status.code(), Some(1));
    let diagnostic = stdout(&output);
    assert!(
        diagnostic.contains("broken.py:1:12-1:13: syntax-error: invalid Python syntax"),
        "{diagnostic}"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn json_output_is_stable_with_zero_rules() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write(&directory.path().join("model.py"), "value = 1\n");

    let output = ddd_lint()
        .current_dir(directory.path())
        .args(["check", "--output-format", "json"])
        .output()
        .expect("ddd-lint should run");

    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "[]\n");
}

#[test]
fn malformed_source_directive_exits_one_with_human_and_json_findings() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write(
        &directory.path().join("model.py"),
        "value = 1  # ddd-lint: ignore[DM001]- missing spaces\n",
    );

    let human = ddd_lint()
        .current_dir(directory.path())
        .arg("check")
        .output()
        .expect("ddd-lint should run");
    assert_eq!(human.status.code(), Some(1));
    assert_eq!(
        stdout(&human),
        "model.py:1:12-1:53: invalid-source-directive: ddd-lint directive requires the exact ` - ` reason delimiter\n"
    );

    let json = ddd_lint()
        .current_dir(directory.path())
        .args(["check", "--output-format", "json"])
        .output()
        .expect("ddd-lint should run");
    assert_eq!(json.status.code(), Some(1));
    let value: serde_json::Value =
        serde_json::from_slice(&json.stdout).expect("valid JSON diagnostics");
    assert_eq!(value[0]["kind"], "invalid-source-directive");
    assert_eq!(value[0]["path"], "model.py");
    assert_eq!(value[0]["range"]["start"]["line"], 1);
    assert_eq!(value[0]["range"]["start"]["column"], 12);
}

#[test]
fn valid_source_directives_stay_clean_with_zero_rules() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write(
        &directory.path().join("model.py"),
        "#!/usr/bin/env python\n# coding: utf-8\n# ddd-lint: file-ignore[DM003] - generated schema\n# ddd-lint: ignore[DM001] - decorated declaration\n@decorator\ndef build():\n    value = (\n        1  # ddd-lint: ignore[CT003] - protocol branch\n        + 2\n    )\n    return value\n",
    );

    let output = ddd_lint()
        .current_dir(directory.path())
        .arg("check")
        .output()
        .expect("ddd-lint should run");
    assert!(output.status.success(), "{}", stdout(&output));
    assert!(output.stdout.is_empty());
}

#[test]
fn malformed_stub_directives_are_excluded() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write(
        &directory.path().join("model.pyi"),
        "# ddd-lint: ignore[UNKNOWN]\nvalue: int\n",
    );

    let output = ddd_lint()
        .current_dir(directory.path())
        .arg("check")
        .output()
        .expect("ddd-lint should run");
    assert!(output.status.success(), "{}", stdout(&output));
    assert!(output.stdout.is_empty());
}

#[test]
fn invalid_per_file_exception_exits_two() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write(
        &directory.path().join("pyproject.toml"),
        "[tool.ddd-lint]\n[[tool.ddd-lint.per-file-ignores]]\npattern = \"tests/**\"\nrules = [\"ALL\"]\nreason = \"Too broad\"\n",
    );

    let output = ddd_lint()
        .current_dir(directory.path())
        .arg("check")
        .output()
        .expect("ddd-lint should run");
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("expected a full rule code"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn resolved_config_expands_selectors_and_prints_rule_and_exception_settings() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write(
        &directory.path().join("pyproject.toml"),
        "[tool.ddd-lint]\nselect = [\"DM\", \"DI001\"]\nignore = [\"DM003\"]\n[tool.ddd-lint.rules.DM002]\nmin-nullable-fields = 4\nmin-nullable-ratio = 0.5\n[tool.ddd-lint.rules.CT003]\nmode = \"strict\"\n[[tool.ddd-lint.per-file-ignores]]\npattern = \"tests/**\"\nrules = [\"DM001\"]\nreason = \"External payload fixture\"\n",
    );

    let output = ddd_lint()
        .current_dir(directory.path())
        .arg("config")
        .output()
        .expect("ddd-lint should run");
    assert!(output.status.success(), "{}", stderr(&output));
    let rendered = stdout(&output);
    assert!(
        rendered.contains("enabled-rules = [\"DM001\", \"DM002\", \"DI001\"]"),
        "{rendered}"
    );
    assert!(rendered.contains("min-nullable-fields = 4"), "{rendered}");
    assert!(rendered.contains("min-nullable-ratio = 0.5"), "{rendered}");
    assert!(
        rendered.contains("[rules.CT003]\nmode = \"strict\""),
        "{rendered}"
    );
    assert!(rendered.contains("pattern = \"tests/**\""), "{rendered}");
    assert!(
        rendered.contains("reason = \"External payload fixture\""),
        "{rendered}"
    );
}

#[test]
fn configurable_rule_prints_registered_defaults() {
    let output = run(&["rule", "DM002"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        stdout(&output)
            .contains("Configuration:\n  min-nullable-fields = 3\n  min-nullable-ratio = 0.30\n")
    );
    let output = run(&["rule", "CT003"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("Configuration:\n  mode = \"dispatch\"\n"));
}

#[test]
fn malformed_directive_survives_syntax_error_guard() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write(
        &directory.path().join("broken.py"),
        "# ddd-lint: ignore[DM001]- malformed\ndef broken(:\n",
    );

    let output = ddd_lint()
        .current_dir(directory.path())
        .arg("check")
        .output()
        .expect("ddd-lint should run");

    assert_eq!(output.status.code(), Some(1));
    let findings = stdout(&output);
    assert!(
        findings.contains(
            "invalid-source-directive: ddd-lint directive requires the exact ` - ` reason delimiter"
        ),
        "{findings}"
    );
    assert!(
        findings.contains("syntax-error: invalid Python syntax"),
        "{findings}"
    );
}

#[test]
fn misplaced_directive_survives_syntax_error_guard() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write(
        &directory.path().join("broken.py"),
        "# ddd-lint: ignore[DM001] - reason\n\ndef broken(:\n",
    );

    let output = ddd_lint()
        .current_dir(directory.path())
        .arg("check")
        .output()
        .expect("ddd-lint should run");

    assert_eq!(output.status.code(), Some(1));
    let findings = stdout(&output);
    assert!(
        findings.contains("invalid-source-directive: own-line ignore must immediately precede a logical statement without a blank line or unrelated comment"),
        "{findings}"
    );
    assert!(
        findings.contains("syntax-error: invalid Python syntax"),
        "{findings}"
    );
}

#[test]
fn ct001_and_ct002_report_missing_members_and_catch_all_independently() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write(
        &directory.path().join("pyproject.toml"),
        "[tool.ddd-lint]\nselect = [\"CT001\", \"CT002\"]\n",
    );
    write(
        &directory.path().join("model.py"),
        "class Draft: pass\nclass Published: pass\nDocument = Draft | Published\ndef label(document: Document) -> str:\n    match document:\n        case Draft():\n            return \"draft\"\n        case _:\n            return \"other\"\n",
    );

    let output = ddd_lint()
        .current_dir(directory.path())
        .arg("check")
        .output()
        .expect("ddd-lint should run");
    assert_eq!(output.status.code(), Some(1));
    let findings = stdout(&output);
    assert!(
        findings
            .contains("CT001 non-exhaustive-match: match omits closed members: model.Published"),
        "{findings}"
    );
    assert!(
        findings.contains("CT002 catch-all-on-closed-type"),
        "{findings}"
    );

    write(
        &directory.path().join("pyproject.toml"),
        "[tool.ddd-lint]\nselect = [\"CT001\"]\n",
    );
    let ct001 = ddd_lint()
        .current_dir(directory.path())
        .arg("check")
        .output()
        .expect("ddd-lint should run");
    assert!(!stdout(&ct001).contains("CT002"));
}

#[test]
fn ct002_allows_assert_never_capture_for_exhaustiveness() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write(
        &directory.path().join("pyproject.toml"),
        "[tool.ddd-lint]\nselect = [\"CT001\", \"CT002\"]\n",
    );
    write(
        &directory.path().join("model.py"),
        "from typing import assert_never\nclass Epic: pass\nclass Assigned: pass\nWorkstream = Epic | Assigned\ndef key(workstream: Workstream) -> str:\n    match workstream:\n        case Epic(): return \"epic\"\n        case Assigned(): return \"assigned\"\n        case unexpected:\n            assert_never(unexpected)\n",
    );
    let output = ddd_lint()
        .current_dir(directory.path())
        .arg("check")
        .output()
        .expect("ddd-lint should run");
    assert!(output.status.success(), "{}", stdout(&output));
    assert!(output.stdout.is_empty());
}

#[test]
fn ct002_resolves_assert_never_and_rejects_guarded_or_local_impostors() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write(
        &directory.path().join("pyproject.toml"),
        "[tool.ddd-lint]\nselect = [\"CT002\"]\n",
    );
    write(
        &directory.path().join("model.py"),
        "import typing\nfrom typing_extensions import assert_never as never\nclass A: pass\nclass B: pass\nValue = A | B\ndef qualified(value: Value):\n    match value:\n        case A(): pass\n        case B(): pass\n        case unexpected: typing.assert_never(unexpected)\ndef aliased(value: Value):\n    match value:\n        case A(): pass\n        case B(): pass\n        case unexpected: never(unexpected)\ndef guarded(value: Value):\n    match value:\n        case unexpected if ready: typing.assert_never(unexpected)\ndef impostor(value: Value):\n    def assert_never(item): pass\n    match value:\n        case unexpected: assert_never(unexpected)\n",
    );
    let output = ddd_lint()
        .current_dir(directory.path())
        .arg("check")
        .output()
        .expect("ddd-lint should run");
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        stdout(&output).matches("CT002").count(),
        2,
        "{}",
        stdout(&output)
    );
}

#[test]
fn ct002_allows_wildcard_assert_never_on_names_and_attribute_chains() {
    // Issue #3: exhaustive enum and class-union matches delegate to imported
    // assert_never. Checking names or matching attribute chains exits cleanly,
    // including aliases, qualified imports, and parenthesized expressions.
    for (import, sink) in [
        ("from typing import assert_never", "assert_never"),
        ("from typing import assert_never as never", "never"),
        ("import typing", "typing.assert_never"),
        ("from typing_extensions import assert_never", "assert_never"),
        (
            "from typing_extensions import assert_never as never",
            "never",
        ),
        ("import typing_extensions as te", "te.assert_never"),
    ] {
        let directory = tempfile::tempdir().expect("temporary directory");
        write(
            &directory.path().join("pyproject.toml"),
            "[tool.ddd-lint]\nselect = [\"CT001\", \"CT002\"]\n",
        );
        write(
            &directory.path().join("model.py"),
            &format!(
                r#"{import}
from enum import Enum
class Status(Enum):
    OPEN = "open"
    CLOSED = "closed"
class Draft: pass
class Published: pass
Document = Draft | Published
class Box:
    status: Status
    document: Document
class Envelope:
    box: Box
def enum_name(status: Status):
    match status:
        case Status.OPEN: pass
        case Status.CLOSED: pass
        case _: {sink}(status)
def union_name(document: Document):
    match document:
        case Draft(): pass
        case Published(): pass
        case _: {sink}(document)
def enum_attribute(box: Box):
    match box.status:
        case Status.OPEN: pass
        case Status.CLOSED: pass
        case _: {sink}((box).status)
def union_attribute(envelope: Envelope):
    match (envelope.box).document:
        case Draft(): pass
        case Published(): pass
        case _: {sink}((envelope.box.document))
"#
            ),
        );
        let output = ddd_lint()
            .current_dir(directory.path())
            .arg("check")
            .output()
            .expect("ddd-lint should run");
        assert!(
            output.status.success(),
            "{import}: {}{}",
            stdout(&output),
            stderr(&output)
        );
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn ct001_reports_new_members_despite_wildcard_assert_never() {
    // Adding enum and union members to previously exhaustive matches must emit
    // CT001 for both missing cases, while their wildcard sinks remain CT002-free.
    let directory = tempfile::tempdir().expect("temporary directory");
    write(
        &directory.path().join("pyproject.toml"),
        "[tool.ddd-lint]\nselect = [\"CT001\", \"CT002\"]\n",
    );
    let source = r#"from enum import Enum
from typing import assert_never
class Status(Enum):
    OPEN = "open"
    CLOSED = "closed"
class Draft: pass
class Published: pass
class Archived: pass
Document = Draft | Published
def enum_label(status: Status):
    match status:
        case Status.OPEN: pass
        case Status.CLOSED: pass
        case _: assert_never(status)
def union_label(document: Document):
    match document:
        case Draft(): pass
        case Published(): pass
        case _: assert_never(document)
"#;
    write(&directory.path().join("model.py"), source);
    let before = ddd_lint()
        .current_dir(directory.path())
        .arg("check")
        .output()
        .expect("ddd-lint should run");
    assert!(before.status.success(), "{}", stdout(&before));
    assert!(before.stdout.is_empty());

    let extended = source
        .replace(
            "    CLOSED = \"closed\"",
            "    CLOSED = \"closed\"\n    ARCHIVED = \"archived\"",
        )
        .replace(
            "Document = Draft | Published",
            "Document = Draft | Published | Archived",
        );
    write(&directory.path().join("model.py"), &extended);
    let after = ddd_lint()
        .current_dir(directory.path())
        .arg("check")
        .output()
        .expect("ddd-lint should run");
    let findings = stdout(&after);
    assert_eq!(after.status.code(), Some(1), "{findings}");
    assert_eq!(findings.matches("CT001").count(), 2, "{findings}");
    assert!(findings.contains("model.Status.ARCHIVED"), "{findings}");
    assert!(findings.contains("model.Archived"), "{findings}");
    assert!(!findings.contains("CT002"), "{findings}");
}

#[test]
fn ct002_rejects_wildcards_without_a_resolved_sink_on_the_same_subject() {
    // With every enum member covered, each unsupported fallback still emits
    // exactly CT002: ordinary/guarded bodies, shadowed callees, wrong arguments,
    // extra statements, or repeated calls that need not return the matched value.
    for (setup, subject, fallback) in [
        ("", "status", "case _: pass"),
        ("", "status", "case _ if ready: assert_never(status)"),
        (
            "    def assert_never(value): pass\n",
            "status",
            "case _: assert_never(status)",
        ),
        (
            "    assert_never = print\n",
            "status",
            "case _: assert_never(status)",
        ),
        (
            "    typing = other\n",
            "status",
            "case _: typing.assert_never(status)",
        ),
        (
            "    typing = other\n",
            "status",
            "case unexpected: typing.assert_never(unexpected)",
        ),
        ("", "status", "case _: assert_never(other)"),
        ("", "box.status", "case _: assert_never(other_box.status)"),
        ("", "box.status", "case _: assert_never(box.other)"),
        ("", "status", "case _: assert_never(status, other)"),
        ("", "status", "case _: assert_never(value=status)"),
        ("", "status", "case _: assert_never(*status)"),
        ("", "status", "case _: assert_never(status, **options)"),
        ("", "status", "case _: print(status); assert_never(status)"),
        ("", "status", "case _: assert_never(status); print(status)"),
        ("", "status", "case _: return assert_never(status)"),
        ("", "get_status()", "case _: assert_never(get_status())"),
        (
            "",
            "get_box().status",
            "case _: assert_never(get_box().status)",
        ),
    ] {
        let directory = tempfile::tempdir().expect("temporary directory");
        write(
            &directory.path().join("pyproject.toml"),
            "[tool.ddd-lint]\nselect = [\"CT001\", \"CT002\"]\n",
        );
        write(
            &directory.path().join("model.py"),
            &format!(
                r#"from enum import Enum
from typing import assert_never
import typing
class Status(Enum):
    OPEN = "open"
    CLOSED = "closed"
class Box:
    status: Status
    other: Status
def get_status() -> Status: ...
def get_box() -> Box: ...
def label(status: Status, other: Status, box: Box, other_box: Box):
{setup}    match {subject}:
        case Status.OPEN: pass
        case Status.CLOSED: pass
        {fallback}
"#
            ),
        );
        let output = ddd_lint()
            .current_dir(directory.path())
            .args(["check", "--output-format", "json"])
            .output()
            .expect("ddd-lint should run");
        let findings = stdout(&output);
        assert_eq!(
            output.status.code(),
            Some(1),
            "{setup}{subject}: {fallback}: {findings}{}",
            stderr(&output)
        );
        let diagnostics: serde_json::Value =
            serde_json::from_str(&findings).expect("JSON diagnostics");
        let diagnostics = diagnostics.as_array().expect("diagnostic array");
        assert_eq!(
            diagnostics.len(),
            1,
            "{setup}{subject}: {fallback}: {findings}"
        );
        assert_eq!(diagnostics[0]["code"], "CT002", "{findings}");
    }
}

#[test]
fn ct001_handles_or_as_guards_literals_enums_and_none() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write(
        &directory.path().join("pyproject.toml"),
        "[tool.ddd-lint]\nselect = [\"CT001\"]\n",
    );
    write(
        &directory.path().join("model.py"),
        "from enum import Enum\nfrom typing import Literal\nclass Draft: pass\nclass Published: pass\nDocument = Draft | Published\nMaybeDocument = Draft | Published | None\nclass Status(Enum):\n    OPEN = \"open\"\n    CLOSED = \"closed\"\ndef complete(document: Document):\n    match document:\n        case (Draft() | Published()) as selected:\n            return selected\ndef guarded(document: Document):\n    match document:\n        case Draft():\n            return 1\n        case Published() if ready:\n            return 2\ndef nullable(document: MaybeDocument):\n    match document:\n        case Draft() | Published():\n            return 1\ndef enum_label(status: Status):\n    match status:\n        case Status.OPEN:\n            return \"open\"\ndef literal_label(value: Literal[\"a\", \"b\", None]):\n    match value:\n        case \"a\":\n            return 1\n        case None:\n            return 0\n",
    );

    let output = ddd_lint()
        .current_dir(directory.path())
        .arg("check")
        .output()
        .expect("ddd-lint should run");
    assert_eq!(output.status.code(), Some(1));
    let findings = stdout(&output);
    assert_eq!(
        findings.matches("CT001 non-exhaustive-match").count(),
        4,
        "{findings}"
    );
    assert!(findings.contains("model.Published"), "{findings}");
    assert!(findings.contains("model.Status.CLOSED"), "{findings}");
    assert!(findings.contains("\"b\""), "{findings}");
    assert!(findings.contains("None"), "{findings}");
}

#[test]
fn ct003_strict_recognizes_documented_direct_tests_but_skips_out_of_scope_forms() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write(
        &directory.path().join("pyproject.toml"),
        "[tool.ddd-lint]\nselect = [\"CT003\"]\n[tool.ddd-lint.rules.CT003]\nmode = \"strict\"\n",
    );
    write(
        &directory.path().join("model.py"),
        "from enum import Enum\nfrom typing import Literal\nclass Draft:\n    kind: Literal[\"draft\"]\nclass Published:\n    kind: Literal[\"published\"]\nDocument = Draft | Published\nclass Status(Enum):\n    OPEN = \"open\"\n    CLOSED = \"closed\"\ndef dispatch(document: Document, status: Status, tag: Literal[\"a\", \"b\"]):\n    if not isinstance(document, Draft):\n        pass\n    if Published is type(document):\n        pass\n    message = \"open\" if status == Status.OPEN else \"closed\"\n    if document.kind == \"draft\":\n        pass\n    if \"a\" == tag:\n        pass\n    while isinstance(document, Draft):\n        break\n    assert status == Status.OPEN\n    if tag in {\"a\", \"b\"}:\n        pass\n    if is_draft(document):\n        pass\ndef shadowed(document: Document, isinstance, type):\n    if isinstance(document, Draft):\n        pass\n    if type(document) is Draft:\n        pass\n",
    );

    let output = ddd_lint()
        .current_dir(directory.path())
        .arg("check")
        .output()
        .expect("ddd-lint should run");
    assert_eq!(output.status.code(), Some(1));
    let findings = stdout(&output);
    assert_eq!(
        findings
            .matches("CT003 conditional-closed-type-dispatch")
            .count(),
        5,
        "{findings}"
    );
}

#[test]
fn ct004_reports_discriminator_match_across_modules() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write(
        &directory.path().join("pyproject.toml"),
        "[tool.ddd-lint]\nselect = [\"CT004\"]\n",
    );
    write(
        &directory.path().join("domain.py"),
        "from typing import Literal\nclass Draft:\n    kind: Literal[\"draft\"]\nclass Published:\n    kind: Literal[\"published\"]\nDocument = Draft | Published\n",
    );
    write(
        &directory.path().join("app.py"),
        "from domain import Document\ndef label(document: Document):\n    match document.kind:\n        case \"draft\": return 1\n        case \"published\": return 2\n",
    );

    let output = ddd_lint()
        .current_dir(directory.path())
        .args(["check", "app.py"])
        .output()
        .expect("ddd-lint should run");
    assert_eq!(output.status.code(), Some(1));
    let findings = stdout(&output);
    assert!(findings.contains("app.py"), "{findings}");
    assert!(findings.contains("CT004 discriminator-match"), "{findings}");
    assert!(!findings.contains("domain.py"), "{findings}");
}

#[test]
fn ct_findings_respect_source_and_per_file_design_exceptions() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write(
        &directory.path().join("pyproject.toml"),
        "[tool.ddd-lint]\nselect = [\"CT001\", \"CT002\"]\n[[tool.ddd-lint.per-file-ignores]]\npattern = \"generated.py\"\nrules = [\"CT001\"]\nreason = \"Generated partial dispatcher\"\n",
    );
    let source = "class Draft: pass\nclass Published: pass\nDocument = Draft | Published\ndef label(document: Document):\n    # ddd-lint: ignore[CT001] - upstream filters published values\n    match document:\n        case Draft(): return 1\n        case _:  # ddd-lint: ignore[CT002] - legacy values need quarantine\n            return 0\n";
    write(&directory.path().join("model.py"), source);
    write(
        &directory.path().join("generated.py"),
        "from model import Document, Draft\ndef label(document: Document):\n    match document:\n        case Draft(): return 1\n",
    );

    let output = ddd_lint()
        .current_dir(directory.path())
        .arg("check")
        .output()
        .expect("ddd-lint should run");
    assert!(output.status.success(), "{}", stdout(&output));
    assert!(output.stdout.is_empty());
}

#[test]
fn ct_rules_resolve_nested_function_scopes() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write(
        &directory.path().join("pyproject.toml"),
        "[tool.ddd-lint]\nselect = [\"CT001\"]\n",
    );
    write(
        &directory.path().join("model.py"),
        "class Draft: pass\nclass Published: pass\nDocument = Draft | Published\ndef outer(enabled: bool):\n    if enabled:\n        def label(document: Document):\n            match document:\n                case Draft(): return 1\n        return label\n",
    );

    let output = ddd_lint()
        .current_dir(directory.path())
        .arg("check")
        .output()
        .expect("ddd-lint should run");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stdout(&output).contains("model.Published"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn ct003_skips_module_shadowed_builtins() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write(
        &directory.path().join("pyproject.toml"),
        "[tool.ddd-lint]\nselect = [\"CT003\"]\n[tool.ddd-lint.rules.CT003]\nmode = \"strict\"\n",
    );
    write(
        &directory.path().join("model.py"),
        "class Draft: pass\nclass Published: pass\nDocument = Draft | Published\nisinstance = vendor_check\ntype = vendor_type\ndef dispatch(document: Document):\n    if isinstance(document, Draft):\n        pass\n    if type(document) is Published:\n        pass\n",
    );

    let output = ddd_lint()
        .current_dir(directory.path())
        .arg("check")
        .output()
        .expect("ddd-lint should run");
    assert!(output.status.success(), "{}", stdout(&output));
    assert!(output.stdout.is_empty());
}

#[test]
fn ct003_skips_function_and_enclosing_scope_shadowed_builtins() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write(
        &directory.path().join("pyproject.toml"),
        "[tool.ddd-lint]\nselect = [\"CT003\"]\n[tool.ddd-lint.rules.CT003]\nmode = \"strict\"\n",
    );
    write(
        &directory.path().join("model.py"),
        "class Draft: pass\nclass Published: pass\nDocument = Draft | Published\ndef outer(enabled: bool):\n    if enabled:\n        from vendor import isinstance\n        def type(value): return Published\n    def inner(document: Document):\n        if isinstance(document, Draft):\n            pass\n        if type(document) is Draft:\n            pass\n    return inner\n",
    );

    let output = ddd_lint()
        .current_dir(directory.path())
        .arg("check")
        .output()
        .expect("ddd-lint should run");
    assert!(output.status.success(), "{}", stdout(&output));
    assert!(output.stdout.is_empty());
}

#[test]
fn ct003_does_not_treat_global_declarations_as_local_shadowing() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write(
        &directory.path().join("pyproject.toml"),
        "[tool.ddd-lint]\nselect = [\"CT003\"]\n[tool.ddd-lint.rules.CT003]\nmode = \"strict\"\n",
    );
    write(
        &directory.path().join("model.py"),
        "class Draft: pass\nclass Published: pass\nDocument = Draft | Published\ndef dispatch(document: Document):\n    global isinstance\n    if isinstance(document, Draft):\n        pass\n    isinstance = vendor_check\n",
    );

    let output = ddd_lint()
        .current_dir(directory.path())
        .arg("check")
        .output()
        .expect("ddd-lint should run");
    assert_eq!(output.status.code(), Some(1));
    assert!(stdout(&output).contains("CT003"), "{}", stdout(&output));
}

#[test]
fn ct_rules_index_definitions_inside_module_and_class_suites() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write(
        &directory.path().join("pyproject.toml"),
        "[tool.ddd-lint]\nselect = [\"CT001\", \"CT003\"]\n",
    );
    write(
        &directory.path().join("model.py"),
        "class Draft: pass\nclass Published: pass\nDocument = Draft | Published\nif enabled:\n    def dispatch(document: Document):\n        if isinstance(document, Draft):\n            return 1\n        elif isinstance(document, Published):\n            return 2\nclass Adapter:\n    if enabled:\n        def label(self, document: Document):\n            match document:\n                case Draft(): return 1\n",
    );

    let output = ddd_lint()
        .current_dir(directory.path())
        .arg("check")
        .output()
        .expect("ddd-lint should run");
    assert_eq!(output.status.code(), Some(1));
    let findings = stdout(&output);
    assert!(findings.contains("CT001"), "{findings}");
    assert!(findings.contains("CT003"), "{findings}");
}

#[test]
fn ct_rules_analyze_async_functions() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write(
        &directory.path().join("pyproject.toml"),
        "[tool.ddd-lint]\nselect = [\"CT001\", \"CT003\"]\n",
    );
    write(
        &directory.path().join("model.py"),
        "class Draft: pass\nclass Published: pass\nDocument = Draft | Published\nasync def label(document: Document):\n    if isinstance(document, Draft):\n        return 1\n    if isinstance(document, Published):\n        return 2\n    match document:\n        case Draft(): return 1\n",
    );

    let output = ddd_lint()
        .current_dir(directory.path())
        .arg("check")
        .output()
        .expect("ddd-lint should run");
    assert_eq!(output.status.code(), Some(1));
    let findings = stdout(&output);
    assert!(findings.contains("CT001"), "{findings}");
    assert!(findings.contains("CT003"), "{findings}");
}

#[test]
fn dm_rules_cover_nullable_thresholds_defaults_and_exclusions() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write(
        &directory.path().join("pyproject.toml"),
        "[tool.ddd-lint]\nselect = [\"DM001\", \"DM002\", \"DM003\"]\n",
    );
    write(
        &directory.path().join("model.py"),
        "from typing import Annotated, Any, ClassVar, Optional\nfrom pydantic import BaseModel, Field, RootModel\nclass Record(BaseModel):\n    first: str | None\n    second: Optional[int]\n    third: \"bytes | None\"\n    fourth: float | None\n    nested: list[str | None]\n    anything: Any\n    opaque: object\n    default_literal: int = 1\n    default_name: int = DEFAULT\n    default_call: list[str] = Field(default_factory=list)\n    metadata_default: Annotated[int, Field(default=0)]\n    required_field: int = Field(...)\n    metadata_only: Annotated[int, Field(description=\"required\")]\n    _private: str | None\n    kind: ClassVar[str] = \"record\"\nclass Child(Record):\n    own: str\nclass Root(RootModel[str | None]):\n    pass\n",
    );

    let output = ddd_lint()
        .current_dir(directory.path())
        .arg("check")
        .output()
        .expect("ddd-lint should run");
    assert_eq!(output.status.code(), Some(1));
    let findings = stdout(&output);
    assert_eq!(
        findings.matches("DM001 nullable-model-field").count(),
        4,
        "{findings}"
    );
    assert_eq!(
        findings.matches("DM002 nullable-heavy-model").count(),
        1,
        "{findings}"
    );
    assert_eq!(
        findings.matches("DM003 defaulted-model-field").count(),
        4,
        "{findings}"
    );
    assert!(!findings.contains("nested` explicitly"), "{findings}");
    assert!(!findings.contains("required_field` has"), "{findings}");
}

#[test]
fn dm002_requires_both_configured_gates_and_dm_suppressions_are_exact() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write(
        &directory.path().join("pyproject.toml"),
        "[tool.ddd-lint]\nselect = [\"DM001\", \"DM002\"]\n[tool.ddd-lint.rules.DM002]\nmin-nullable-fields = 2\nmin-nullable-ratio = 0.60\n",
    );
    write(
        &directory.path().join("model.py"),
        "from pydantic import BaseModel\n# ddd-lint: ignore[DM002] - sparse transport record\nclass Record(BaseModel):\n    first: str | None  # ddd-lint: ignore[DM001] - absence is meaningful\n    second: str | None\n    required: str\n",
    );
    let output = ddd_lint()
        .current_dir(directory.path())
        .arg("check")
        .output()
        .expect("ddd-lint should run");
    assert_eq!(output.status.code(), Some(1));
    let findings = stdout(&output);
    assert_eq!(findings.matches("DM001").count(), 1, "{findings}");
    assert!(!findings.contains("DM002"), "{findings}");
}

#[test]
fn dm_rules_resolve_cross_file_model_aliases_and_documented_field_forms() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write(
        &directory.path().join("pyproject.toml"),
        "[tool.ddd-lint]\nselect = [\"DM001\", \"DM003\"]\n",
    );
    write(
        &directory.path().join("base.py"),
        "from pydantic import BaseModel as Model\nclass DomainModel(Model):\n    inherited: str\n",
    );
    write(
        &directory.path().join("model.py"),
        "from typing import Union\nfrom pydantic import Field, computed_field\nfrom base import DomainModel as Base\nclass Shipment(Base):\n    tracking: Union[str, None]\n    priority: int = Field(default=0)\n    @computed_field\n    def label(self) -> str:\n        return self.tracking or \"pending\"\n",
    );
    let output = ddd_lint()
        .current_dir(directory.path())
        .arg("check")
        .output()
        .expect("ddd-lint should run");
    assert_eq!(output.status.code(), Some(1));
    let findings = stdout(&output);
    assert_eq!(findings.matches("DM001").count(), 1, "{findings}");
    assert_eq!(findings.matches("DM003").count(), 1, "{findings}");
    assert!(!findings.contains("inherited`"), "{findings}");
    assert!(!findings.contains("label`"), "{findings}");
}

#[test]
fn di001_detects_every_documented_hidden_constructor_fallback_form() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write(
        &directory.path().join("pyproject.toml"),
        "[tool.ddd-lint]\nselect = [\"DI001\"]\n",
    );
    let source = "class NoopRepository: pass\nclass SystemClock: pass\nclass DefaultGateway: pass\nclass Direct:\n    def __init__(self, repository=NoopRepository()):\n        self.repository = repository\nclass OrFallback:\n    def __init__(self, clock=None):\n        self.clock = clock or SystemClock()\nclass ConditionalFallback:\n    def __init__(self, gateway=None):\n        self.gateway = gateway if gateway is not None else DefaultGateway()\nclass GuardedFallback:\n    def __init__(self, repository=None):\n        if repository is None:\n            repository = NoopRepository()\n        self.repository = repository\n";
    write(&directory.path().join("model.py"), source);

    let output = ddd_lint()
        .current_dir(directory.path())
        .args(["check", "--output-format", "json"])
        .output()
        .expect("ddd-lint should run");

    assert_eq!(output.status.code(), Some(1));
    let diagnostics: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("valid JSON diagnostics");
    let diagnostics = diagnostics.as_array().expect("diagnostic array");
    assert_eq!(diagnostics.len(), 4, "{}", stdout(&output));
    let expected = [
        ("NoopRepository()", 5),
        ("SystemClock()", 9),
        ("DefaultGateway()", 12),
        ("NoopRepository()", 16),
    ];
    for (diagnostic, (call, line)) in diagnostics.iter().zip(expected) {
        assert_eq!(diagnostic["code"], "DI001");
        assert_eq!(diagnostic["range"]["start"]["line"], line);
        let source_line = source.lines().nth(line - 1).expect("source line");
        assert_eq!(
            diagnostic["range"]["start"]["column"],
            source_line.find(call).expect("call on line") + 1
        );
    }
}

#[test]
fn di001_is_limited_to_init_and_accepts_explicit_injection_and_plain_defaults() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write(
        &directory.path().join("pyproject.toml"),
        "[tool.ddd-lint]\nselect = [\"DI001\"]\n",
    );
    write(
        &directory.path().join("model.py"),
        "DEFAULT_REPOSITORY = object()\nclass NoopRepository: pass\nclass InMemoryRepository: pass\ndef ordinary(repository=NoopRepository()): return repository\nclass Service:\n    def __init__(self, repository, retries=3, default=DEFAULT_REPOSITORY):\n        self.repository = repository\n        self.retries = retries\n        self.default = default\n    def __new__(cls, repository=NoopRepository()): return super().__new__(cls)\n    @classmethod\n    def create(cls, repository=NoopRepository()): return cls(repository)\nclass AuditOnly:\n    def __init__(self, repository=None):\n        if repository is None:\n            self.audit.repository = NoopRepository()\n        self.repository = repository\nservice = Service(repository=InMemoryRepository())\nnoop = Service(repository=NoopRepository())\n",
    );

    let output = ddd_lint()
        .current_dir(directory.path())
        .arg("check")
        .output()
        .expect("ddd-lint should run");

    assert!(output.status.success(), "{}", stdout(&output));
    assert!(output.stdout.is_empty());
}

#[test]
fn di001_respects_source_design_exception_at_fallback_call() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write(
        &directory.path().join("pyproject.toml"),
        "[tool.ddd-lint]\nselect = [\"DI001\"]\n",
    );
    write(
        &directory.path().join("model.py"),
        "class SystemClock: pass\nclass LocalPreview:\n    def __init__(self, clock=None):\n        self.clock = clock or SystemClock()  # ddd-lint: ignore[DI001] - Preview is a self-contained process boundary\nclass EagerPreview:\n    def __init__(self, clock=SystemClock()):  # ddd-lint: ignore[DI001] - Definition-time singleton is intentional\n        self.clock = clock\n",
    );

    let output = ddd_lint()
        .current_dir(directory.path())
        .arg("check")
        .output()
        .expect("ddd-lint should run");

    assert!(output.status.success(), "{}", stdout(&output));
}

#[test]
fn sup001_evaluates_source_directives_per_code_against_enabled_diagnostics() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write(
        &directory.path().join("pyproject.toml"),
        "[tool.ddd-lint]\nselect = [\"DM001\", \"DM003\", \"SUP001\"]\n",
    );
    write(
        &directory.path().join("model.py"),
        "from pydantic import BaseModel\nclass Customer(BaseModel):\n    name: str | None  # ddd-lint: ignore[DM001, DM003] - Imported records may omit names\n",
    );

    let output = ddd_lint()
        .current_dir(directory.path())
        .arg("check")
        .output()
        .expect("ddd-lint should run");

    assert_eq!(output.status.code(), Some(1));
    let findings = stdout(&output);
    assert_eq!(
        findings.matches("SUP001 unused-suppression").count(),
        1,
        "{findings}"
    );
    assert!(findings.contains("model.py:3:23-3:"), "{findings}");
    assert!(findings.contains("exception for DM003"), "{findings}");
    assert!(!findings.contains("exception for DM001"), "{findings}");
    assert!(
        !findings.contains("DM001 nullable-model-field"),
        "{findings}"
    );
}

#[test]
fn sup001_treats_suppressions_for_unselected_rules_as_unused() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write(
        &directory.path().join("pyproject.toml"),
        "[tool.ddd-lint]\nselect = [\"SUP001\"]\n",
    );
    write(
        &directory.path().join("model.py"),
        "value = 1  # ddd-lint: ignore[DM001] - used by broader policy\n",
    );

    let output = ddd_lint()
        .current_dir(directory.path())
        .arg("check")
        .output()
        .expect("ddd-lint should run");

    assert_eq!(output.status.code(), Some(1));
    let findings = stdout(&output);
    assert_eq!(
        findings.matches("SUP001 unused-suppression").count(),
        1,
        "{findings}"
    );
    assert!(findings.contains("exception for DM001"), "{findings}");
}

#[test]
fn sup001_accepts_used_source_exception_and_reports_documented_stale_example() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write(
        &directory.path().join("pyproject.toml"),
        "[tool.ddd-lint]\nselect = [\"DM001\", \"SUP001\"]\n",
    );
    write(
        &directory.path().join("model.py"),
        "from pydantic import BaseModel\nclass Customer(BaseModel):\n    missing: str | None  # ddd-lint: ignore[DM001] - Absence is domain-significant\n    name: str  # ddd-lint: ignore[DM001] - Imported records may omit names\n",
    );

    let output = ddd_lint()
        .current_dir(directory.path())
        .arg("check")
        .output()
        .expect("ddd-lint should run");

    assert_eq!(output.status.code(), Some(1));
    let findings = stdout(&output);
    assert_eq!(
        findings.matches("SUP001 unused-suppression").count(),
        1,
        "{findings}"
    );
    assert!(findings.contains("model.py:4:"), "{findings}");
}

#[test]
fn sup001_evaluates_structured_config_per_code_and_anchors_rule_values() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write(
        &directory.path().join("pyproject.toml"),
        "[tool.ddd-lint]\nselect = [\"DM001\", \"DM003\", \"SUP001\"]\n[[tool.ddd-lint.per-file-ignores]]\npattern = \"model.py\"\nrules = [\"DM001\", \"DM003\"]\nreason = \"Imported record schema\"\n",
    );
    write(
        &directory.path().join("model.py"),
        "from pydantic import BaseModel\nclass Customer(BaseModel):\n    name: str | None\n",
    );

    let output = ddd_lint()
        .current_dir(directory.path())
        .arg("check")
        .output()
        .expect("ddd-lint should run");

    assert_eq!(output.status.code(), Some(1));
    let findings = stdout(&output);
    assert_eq!(
        findings.matches("SUP001 unused-suppression").count(),
        1,
        "{findings}"
    );
    assert!(findings.contains("pyproject.toml:5:19-5:26"), "{findings}");
    assert!(findings.contains("exception for DM003"), "{findings}");
}

#[test]
fn sup001_reports_every_code_when_config_pattern_matches_no_analyzed_file() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write(
        &directory.path().join("pyproject.toml"),
        "[tool.ddd-lint]\nselect = [\"DM001\", \"DM003\", \"SUP001\"]\n[[tool.ddd-lint.per-file-ignores]]\npattern = \"missing/**\"\nrules = [\"DM001\", \"DM003\"]\nreason = \"Legacy generated models\"\n",
    );
    write(&directory.path().join("model.py"), "value = 1\n");

    let output = ddd_lint()
        .current_dir(directory.path())
        .arg("check")
        .output()
        .expect("ddd-lint should run");

    assert_eq!(output.status.code(), Some(1));
    let findings = stdout(&output);
    assert_eq!(
        findings.matches("SUP001 unused-suppression").count(),
        2,
        "{findings}"
    );
    assert_eq!(
        findings.matches("matching no analyzed file").count(),
        2,
        "{findings}"
    );
    assert!(findings.contains("DM001"), "{findings}");
    assert!(findings.contains("DM003"), "{findings}");
}

#[test]
fn project_ignore_disables_sup001_without_invalidating_other_exceptions() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write(
        &directory.path().join("pyproject.toml"),
        "[tool.ddd-lint]\nselect = [\"DM001\", \"SUP001\"]\nignore = [\"SUP001\"]\n[[tool.ddd-lint.per-file-ignores]]\npattern = \"model.py\"\nrules = [\"DM001\"]\nreason = \"Retained for another policy profile\"\n",
    );
    write(&directory.path().join("model.py"), "value = 1\n");

    let output = ddd_lint()
        .current_dir(directory.path())
        .arg("check")
        .output()
        .expect("ddd-lint should run");

    assert!(output.status.success(), "{}", stdout(&output));
    assert!(output.stdout.is_empty());
}

#[test]
fn sup001_remains_invalid_in_local_source_and_config_exceptions() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write(
        &directory.path().join("model.py"),
        "value = 1  # ddd-lint: ignore[SUP001] - circular suppression\n",
    );
    let source = ddd_lint()
        .current_dir(directory.path())
        .arg("check")
        .output()
        .expect("ddd-lint should run");
    assert_eq!(source.status.code(), Some(1));
    assert!(stdout(&source).contains("cannot suppress SUP001"));

    write(
        &directory.path().join("pyproject.toml"),
        "[tool.ddd-lint]\n[[tool.ddd-lint.per-file-ignores]]\npattern = \"model.py\"\nrules = [\"SUP001\"]\nreason = \"Circular suppression\"\n",
    );
    write(&directory.path().join("model.py"), "value = 1\n");
    let config = ddd_lint()
        .current_dir(directory.path())
        .arg("check")
        .output()
        .expect("ddd-lint should run");
    assert_eq!(config.status.code(), Some(2));
    assert!(stderr(&config).contains("rules cannot contain SUP001"));
}
