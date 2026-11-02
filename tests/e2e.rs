use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const RULES: [&str; 10] = [
    "DM001", "DM002", "DM003", "DM004", "CT001", "CT002", "CT003", "CT004", "DI001", "SUP001",
];

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/e2e")
        .join(name)
}

fn run(name: &str, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ddd-lint"))
        .current_dir(fixture(name))
        .args(arguments)
        .output()
        .expect("ddd-lint fixture should run")
}

fn normalized_snapshot(name: &str, output: &Output) -> String {
    let root = fs::canonicalize(fixture(name)).expect("fixture should canonicalize");
    let root = root.to_string_lossy();
    let stdout = String::from_utf8_lossy(&output.stdout)
        .replace(root.as_ref(), "<FIXTURE>")
        .replace("\r\n", "\n")
        .replace('\\', "/");
    let stderr = String::from_utf8_lossy(&output.stderr)
        .replace(root.as_ref(), "<FIXTURE>")
        .replace("\r\n", "\n")
        .replace('\\', "/");
    format!(
        "exit: {}\nstdout:\n{stdout}stderr:\n{stderr}",
        output
            .status
            .code()
            .expect("process should have an exit code")
    )
}

fn assert_snapshot(snapshot: &str, actual: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/snapshots")
        .join(format!("{snapshot}.snap"));
    if std::env::var_os("DDD_LINT_UPDATE_SNAPSHOTS").is_some() {
        fs::create_dir_all(path.parent().expect("snapshot has parent"))
            .expect("snapshot directory should exist");
        fs::write(&path, actual).expect("snapshot should update");
    }
    let expected = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read snapshot {}: {error}", path.display()));
    assert_eq!(actual, expected, "snapshot {}", path.display());
}

#[test]
fn realistic_projects_match_human_and_json_snapshots() {
    let cases: [(&str, &str, &[&str]); 8] = [
        ("flat", "flat-human", &["check"]),
        ("flat", "flat-json", &["check", "--output-format", "json"]),
        ("src-layout", "src-layout-human", &["check"]),
        (
            "src-layout",
            "src-layout-json",
            &["check", "--output-format", "json"],
        ),
        ("stubs", "stubs-human", &["check"]),
        ("stubs", "stubs-json", &["check", "--output-format", "json"]),
        (
            "workspace",
            "workspace-limited-human",
            &["check", "packages/app/src/app/handlers.py"],
        ),
        (
            "workspace",
            "workspace-limited-json",
            &[
                "check",
                "--output-format",
                "json",
                "packages/app/src/app/handlers.py",
            ],
        ),
    ];

    for (fixture, snapshot, arguments) in cases {
        let output = run(fixture, arguments);
        assert!(
            !output.stdout.contains(&b'\\'),
            "diagnostic paths must use forward slashes: {snapshot}"
        );
        assert_snapshot(snapshot, &normalized_snapshot(fixture, &output));
    }
}

#[test]
fn malformed_inputs_match_end_to_end_snapshots() {
    let cases: [(&str, &str, &[&str]); 4] = [
        ("malformed-sources", "malformed-sources-human", &["check"]),
        (
            "malformed-sources",
            "malformed-sources-json",
            &["check", "--output-format", "json"],
        ),
        ("malformed-toml", "malformed-toml", &["check"]),
        ("malformed-workspace", "malformed-workspace", &["check"]),
    ];

    for (fixture, snapshot, arguments) in cases {
        let output = run(fixture, arguments);
        assert_snapshot(snapshot, &normalized_snapshot(fixture, &output));
    }
}

#[test]
fn repeated_parallel_runs_are_byte_identical() {
    for (fixture, arguments) in [
        ("flat", &["check", "--output-format", "json"][..]),
        (
            "malformed-sources",
            &["check", "--output-format", "json"][..],
        ),
    ] {
        let first = run(fixture, arguments);
        assert_eq!(first.status.code(), Some(1));
        assert!(first.stderr.is_empty());
        for _ in 0..10 {
            let repeated = run(fixture, arguments);
            assert_eq!(repeated.status.code(), first.status.code());
            assert_eq!(repeated.stdout, first.stdout);
            assert_eq!(repeated.stderr, first.stderr);
        }
    }
}

#[test]
fn coverage_matrix_names_every_rule_and_case_kind() {
    let coverage = fs::read_to_string(fixture("COVERAGE.md")).expect("coverage matrix should read");
    for rule in RULES {
        let row = coverage
            .lines()
            .find(|line| line.starts_with(&format!("| {rule} |")))
            .unwrap_or_else(|| panic!("missing coverage row for {rule}"));
        assert_eq!(row.matches('|').count(), 6, "incomplete row for {rule}");
        assert!(!row.contains("|  |"), "empty fixture case for {rule}");
    }
}
