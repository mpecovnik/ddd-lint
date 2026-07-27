use std::path::PathBuf;
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
    let output = run(&["check"]);

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
        "DM001: nullable-model-field\nDocumentation: https://github.com/mpecovnik/ddd-lint/blob/main/docs/rules/dm001-nullable-model-field.md\n"
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
        "# Configuration discovery is not implemented yet.\nselect = []\n"
    );
}
