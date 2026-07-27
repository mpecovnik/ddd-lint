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
        "select = []\nignore = []\nenabled-rules = []\nsrc = [\".\", \"src\"]\ntarget-version = \"py310\"\nexclude = []\n\n[rules.DM002]\nmin-nullable-fields = 3\nmin-nullable-ratio = 0.3\n"
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
        "[tool.ddd-lint]\nselect = [\"DM\", \"DI001\"]\nignore = [\"DM003\"]\n[tool.ddd-lint.rules.DM002]\nmin-nullable-fields = 4\nmin-nullable-ratio = 0.5\n[[tool.ddd-lint.per-file-ignores]]\npattern = \"tests/**\"\nrules = [\"DM001\"]\nreason = \"External payload fixture\"\n",
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
