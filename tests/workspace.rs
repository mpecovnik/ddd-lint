use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn write(path: &Path, source: &str) {
    fs::create_dir_all(path.parent().expect("fixture parent")).expect("fixture directory");
    fs::write(path, source).expect("fixture source");
}

fn copy_fixture(source: &Path, target: &Path) {
    fs::create_dir_all(target).expect("fixture directory");
    for entry in fs::read_dir(source).expect("fixture entries") {
        let entry = entry.expect("fixture entry");
        let destination = target.join(entry.file_name());
        if entry.file_type().expect("fixture file type").is_dir() {
            copy_fixture(&entry.path(), &destination);
        } else {
            fs::copy(entry.path(), destination).expect("fixture copy");
        }
    }
}

fn workspace() -> tempfile::TempDir {
    let directory = tempfile::tempdir().expect("temporary workspace");
    copy_fixture(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/workspace-importlib"),
        directory.path(),
    );
    directory
}

fn check(directory: &Path, path: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ddd-lint"))
        .current_dir(directory)
        .args(["check", path, "--output-format", "json"])
        .output()
        .expect("ddd-lint should run")
}

fn diagnostics(output: &Output) -> Vec<serde_json::Value> {
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("JSON diagnostics")
}

#[test]
fn src_workspace_analyzes_duplicate_test_names_with_independent_local_types() {
    // Issue #2: two src-layout members share tests/conftest.py, test_models.py,
    // and helpers.py. Both test trees are analyzed, local relative imports stay
    // separate, and application imports resolve across members. Findings exit 1,
    // not a duplicate-module configuration error. Repeated output is deterministic.
    let workspace = workspace();
    let output = check(workspace.path(), ".");
    let findings = diagnostics(&output);
    assert_eq!(output.status.code(), Some(1));
    let observed = findings
        .iter()
        .map(|finding| {
            (
                finding["path"].as_str().unwrap(),
                finding["code"].as_str().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        observed,
        [
            ("packages/alpha/tests/conftest.py", "DM004"),
            ("packages/alpha/tests/test_models.py", "DM004"),
            ("packages/beta/src/beta/consumer.py", "CT001"),
            ("packages/beta/tests/conftest.py", "DM004"),
            ("packages/beta/tests/test_models.py", "CT001"),
        ]
    );
    for finding in findings.iter().filter(|finding| finding["code"] == "CT001") {
        assert_eq!(
            finding["message"],
            "match omits closed members: alpha.models.Published"
        );
    }
    let repeated = check(workspace.path(), ".");
    assert_eq!(repeated.status.code(), output.status.code());
    assert_eq!(repeated.stdout, output.stdout);

    // Duplicate test names are valid regardless of selected rules or pytest being run.
    write(
        &workspace.path().join("pyproject.toml"),
        "[tool.uv.workspace]\nmembers = [\"packages/*\"]\n[tool.ddd-lint]\n",
    );
    let disabled = check(workspace.path(), ".");
    assert_eq!(
        disabled.status.code(),
        Some(0),
        "{:?}",
        diagnostics(&disabled)
    );
}

#[test]
fn member_test_only_check_keeps_application_and_relative_import_support() {
    // Checking only beta's tests from that member still indexes alpha's src
    // declarations and beta's helpers, emitting exactly beta's two test findings.
    let workspace = workspace();
    let output = check(&workspace.path().join("packages/beta"), "tests");
    let findings = diagnostics(&output);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(findings.len(), 2, "{findings:?}");
    assert_eq!(findings[0]["path"], "packages/beta/tests/conftest.py");
    assert_eq!(findings[0]["code"], "DM004");
    assert_eq!(findings[1]["path"], "packages/beta/tests/test_models.py");
    assert_eq!(findings[1]["code"], "CT001");
}

#[test]
fn duplicate_application_modules_still_fail_in_src_and_flat_members() {
    // The src-layout fix must not choose arbitrarily between actual application
    // modules. Both layouts retain deterministic duplicate-module errors (exit 2).
    for layout in ["src/", ""] {
        let directory = tempfile::tempdir().expect("temporary workspace");
        write(
            &directory.path().join("pyproject.toml"),
            "[tool.uv.workspace]\nmembers = [\"packages/*\"]\n[tool.ddd-lint]\n",
        );
        for member in ["alpha", "beta"] {
            write(
                &directory
                    .path()
                    .join(format!("packages/{member}/pyproject.toml")),
                &format!("[project]\nname = \"{member}\"\nversion = \"0.1.0\"\n"),
            );
            write(
                &directory
                    .path()
                    .join(format!("packages/{member}/{layout}shared.py")),
                "class Model: pass\n",
            );
        }
        let output = check(directory.path(), ".");
        assert_eq!(output.status.code(), Some(2));
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(
            error.contains("duplicate importable module `shared`"),
            "{error}"
        );
        assert!(
            error.contains(&format!("packages/alpha/{layout}shared.py")),
            "{error}"
        );
        assert!(
            error.contains(&format!("packages/beta/{layout}shared.py")),
            "{error}"
        );
    }
}

#[test]
fn mixed_layouts_keep_flat_imports_and_nested_src_tests_use_workspace_identities() {
    // A flat member remains importable by bare module name. Src-layout members
    // nested beneath two flat members must not inherit their parents' import roots:
    // both nested/tests/conftest.py files need independent workspace identities.
    let workspace = workspace();
    write(
        &workspace.path().join("pyproject.toml"),
        "[tool.uv.workspace]\nmembers = [\"packages/**\"]\n[tool.ddd-lint]\nselect = [\"DM004\"]\n",
    );
    for member in ["gamma", "delta"] {
        write(
            &workspace
                .path()
                .join(format!("packages/{member}/pyproject.toml")),
            &format!("[project]\nname = \"{member}\"\nversion = \"0.1.0\"\n"),
        );
        write(
            &workspace
                .path()
                .join(format!("packages/{member}/nested/pyproject.toml")),
            &format!("[project]\nname = \"{member}-nested\"\nversion = \"0.1.0\"\n"),
        );
        write(
            &workspace
                .path()
                .join(format!("packages/{member}/nested/src/{member}_nested.py")),
            "value = 1\n",
        );
        write(
            &workspace
                .path()
                .join(format!("packages/{member}/nested/tests/conftest.py")),
            "from flat_models import Model\nclass Record(Model):\n    selected_by: str | None\n    selected_at: str | None\n",
        );
    }
    write(
        &workspace.path().join("packages/gamma/flat_models.py"),
        "from alpha.models import Model\n",
    );
    let output = check(workspace.path(), "packages/gamma/nested/tests");
    let findings = diagnostics(&output);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(
        findings[0]["path"],
        "packages/gamma/nested/tests/conftest.py"
    );
    assert_eq!(findings[0]["code"], "DM004");
}

#[test]
fn syntax_errors_in_src_member_tests_are_not_silently_excluded() {
    // A sibling test file remains real analysis input, including its syntax errors.
    // Duplicate conftest names must not mask the normal source-error exit status.
    let workspace = workspace();
    write(
        &workspace.path().join("packages/beta/tests/conftest.py"),
        "def broken(:\n",
    );
    let output = check(workspace.path(), ".");
    let findings = diagnostics(&output);
    assert_eq!(output.status.code(), Some(1));
    assert!(!findings.is_empty());
    for finding in findings {
        assert_eq!(finding["kind"], "syntax-error");
        assert_eq!(finding["path"], "packages/beta/tests/conftest.py");
    }
}
