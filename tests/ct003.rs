use std::fs;
use std::process::{Command, Output};

const DOMAIN: &str = r#"from enum import Enum
from typing import Literal
class Draft:
    kind: Literal["draft"]
class Published:
    kind: Literal["published"]
class Archived:
    kind: Literal["archived"]
Document = Draft | Published | Archived
class State(Enum):
    NOT_EVALUATED = "not_evaluated"
    FAILED = "failed"
    NOT_REQUESTED = "not_requested"
class Box:
    document: Document
    state: State
"#;

fn check(source: &str, config: &str) -> Output {
    let directory = tempfile::tempdir().expect("temporary project");
    fs::write(directory.path().join("domain.py"), DOMAIN).expect("domain fixture");
    fs::write(directory.path().join("app.py"), source).expect("application fixture");
    fs::write(directory.path().join("pyproject.toml"), config).expect("configuration");
    Command::new(env!("CARGO_BIN_EXE_ddd-lint"))
        .current_dir(directory.path())
        .args(["check", "app.py", "--output-format", "json"])
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

const SELECT_CT003: &str = "[tool.ddd-lint]\nselect = [\"CT003\"]\n";

#[test]
fn dispatch_reports_one_finding_for_ladders_and_adjacent_exiting_branches() {
    // Multi-member decisions on one declared value should produce one CT003 at
    // the first test, with or without a fallback. Real first-party imports supply
    // class, enum, literal, and discriminator facts through the CLI project graph.
    for body in [
        "    if isinstance(document, Draft):\n        return 1\n    elif isinstance(document, Published):\n        return 2\n    else:\n        return 3\n",
        "    if isinstance(document, Draft):\n        use(document)\n    elif isinstance(document, Published):\n        use(document)\n",
        "    if isinstance(document, Draft):\n        return 1\n    if isinstance(document, Published):\n        return 2\n    if isinstance(document, Archived):\n        return 3\n",
        "    if state == State.NOT_EVALUATED:\n        return 1\n    if State.FAILED is state:\n        return 2\n    return 3\n",
        "    if type(document) is Draft:\n        return 1\n    elif Published == type(document):\n        return 2\n",
        "    if tag == 'draft':\n        return 1\n    elif 'published' == tag:\n        return 2\n",
        "    if document.kind == 'draft':\n        return 1\n    elif document.kind == 'published':\n        return 2\n",
        "    if isinstance(box.document, Draft):\n        return 1\n    elif isinstance((box).document, Published):\n        return 2\n",
        "    if isinstance(document, Draft):\n        raise ValueError()\n    if isinstance(document, Published):\n        raise TypeError()\n    return 3\n",
    ] {
        let source = format!(
            "from domain import Document, Draft, Published, Archived, State, Box\nfrom typing import Literal\ndef label(document: Document, state: State, box: Box, tag: Literal['draft', 'published', 'archived']):\n{body}"
        );
        let output = check(&source, SELECT_CT003);
        let findings = diagnostics(&output);
        assert_eq!(output.status.code(), Some(1), "{source}\n{findings:?}");
        assert_eq!(findings.len(), 1, "{source}\n{findings:?}");
        assert_eq!(findings[0]["code"], "CT003");
        assert!(
            findings[0]["message"]
                .as_str()
                .unwrap()
                .contains("multiple members")
        );
        assert_eq!(findings[0]["range"]["start"]["line"], 4, "{findings:?}");
    }
}

#[test]
fn dispatch_allows_predicates_and_skips_ambiguous_sequences() {
    // Single guards/groupings and independent actions are intentional predicates.
    // Different subjects, repeated members, compound/negative tests, interrupted
    // runs, uncertain types, and shadowed builtins must not become speculative
    // dispatch findings. Every source is checked through the default CT003 mode.
    for body in [
        "    if isinstance(document, Draft):\n        return 1\n    return 0\n",
        "    if state == State.FAILED:\n        return 1\n    else:\n        return 0\n",
        "    return 30 if state == State.FAILED else 7\n",
        "    if admin or (organization_matches and state == State.FAILED):\n        return document\n",
        "    if not isinstance(document, Draft) and document.shape.intersects(extent):\n        use(document)\n",
        "    if isinstance(document, Draft):\n        record_metric()\n    if isinstance(document, Published):\n        record_metric()\n",
        "    if isinstance(document, Draft):\n        return 1\n    elif isinstance(other, Published):\n        return 2\n",
        "    if isinstance(document, Draft):\n        return 1\n    elif isinstance(document, Draft):\n        return 2\n",
        "    if isinstance(document, Draft):\n        return 1\n    elif isinstance(document, DraftAlias):\n        return 2\n",
        "    if isinstance(document, Draft):\n        return 1\n    if isinstance(other, Published):\n        return 2\n",
        "    if isinstance(document, Draft) or isinstance(document, Published):\n        return 1\n",
        "    if isinstance(document, Draft) and ready:\n        return 1\n    elif isinstance(document, Published):\n        return 2\n",
        "    if not isinstance(document, Draft):\n        return 1\n    elif not isinstance(document, Published):\n        return 2\n",
        "    if state != State.FAILED:\n        return 1\n    elif state != State.NOT_EVALUATED:\n        return 2\n",
        "    if isinstance(document, Draft):\n        return 1\n    document = refresh()\n    if isinstance(document, Published):\n        return 2\n",
        "    if isinstance(document, Draft):\n        if ready:\n            return 1\n    if isinstance(document, Published):\n        return 2\n",
        "    if isinstance(document, Draft):\n        return 1\n    if isinstance(document, Published):\n        record_metric()\n",
        "    if isinstance(unknown, Draft):\n        return 1\n    elif isinstance(unknown, Published):\n        return 2\n",
        "    isinstance = custom_check\n    if isinstance(document, Draft):\n        return 1\n    elif isinstance(document, Published):\n        return 2\n",
        "    type = custom_type\n    if type(document) is Draft:\n        return 1\n    elif type(document) is Published:\n        return 2\n",
        "    if isinstance(current(), Draft):\n        return 1\n    elif isinstance(current(), Published):\n        return 2\n",
    ] {
        let source = format!(
            "from domain import Document, Draft, Published, State\nfrom domain import Draft as DraftAlias\ndef current() -> Document: ...\ndef label(document: Document, other: Document, state: State):\n{body}"
        );
        let output = check(&source, SELECT_CT003);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{source}\n{:?}",
            diagnostics(&output)
        );
        assert!(diagnostics(&output).is_empty());
    }
}

#[test]
fn strict_mode_preserves_single_predicates_and_dispatch_is_explicitly_selectable() {
    // Existing strict users can retain every direct-test finding. Selecting
    // dispatch explicitly has the same low-noise behavior as the new default.
    let source = "from domain import Document, Draft, State\ndef label(document: Document, state: State):\n    if not isinstance(document, Draft) and ready:\n        return 1\n    return 30 if state == State.FAILED else 7\n";
    for (mode, count) in [("strict", 2), ("dispatch", 0)] {
        let output = check(
            source,
            &format!("{SELECT_CT003}[tool.ddd-lint.rules.CT003]\nmode = \"{mode}\"\n"),
        );
        let findings = diagnostics(&output);
        assert_eq!(findings.len(), count, "{mode}: {findings:?}");
        assert_eq!(output.status.code(), Some(if count == 0 { 0 } else { 1 }));
    }
}

#[test]
fn dispatch_respects_nested_suites_and_does_not_join_separate_scopes() {
    // An async nested function inside a try suite has a real dispatch; lone tests
    // in separate functions must not be joined. Exactly that nested dispatch emits.
    let source = r#"from domain import Document, Draft, Published
def outer():
    async def label(document: Document):
        try:
            if isinstance(document, Draft):
                return 1
            if isinstance(document, Published):
                return 2
        finally:
            cleanup()
def first(document: Document):
    if isinstance(document, Draft):
        return 1
def second(document: Document):
    if isinstance(document, Published):
        return 2
"#;
    let output = check(source, SELECT_CT003);
    let findings = diagnostics(&output);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0]["range"]["start"]["line"], 5, "{findings:?}");
}

#[test]
fn dispatch_suppression_covers_whole_sequence_and_old_predicate_exceptions_go_stale() {
    // One reason on the initial if suppresses one dispatch finding. A suppression
    // on a now-allowed single guard becomes unused under SUP001, exposing migration.
    let source = r#"from domain import Document, Draft, Published
def dispatch(document: Document):
    # ddd-lint: ignore[CT003] - External callback protocol requires branching
    if isinstance(document, Draft):
        return 1
    if isinstance(document, Published):
        return 2
def guard(document: Document):
    # ddd-lint: ignore[CT003] - Legacy strict-mode exception
    if isinstance(document, Draft):
        return 1
"#;
    let output = check(
        source,
        "[tool.ddd-lint]\nselect = [\"CT003\", \"SUP001\"]\n",
    );
    let findings = diagnostics(&output);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0]["code"], "SUP001");
    assert_eq!(findings[0]["range"]["start"]["line"], 9, "{findings:?}");
}

#[test]
fn invalid_ct003_mode_is_a_configuration_error() {
    // Misspelling the policy must fail configuration rather than silently select
    // a different rule contract.
    let output = check(
        "",
        &format!("{SELECT_CT003}[tool.ddd-lint.rules.CT003]\nmode = \"strcit\"\n"),
    );
    assert_eq!(output.status.code(), Some(2));
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("strcit"), "{error}");
    assert!(error.contains("dispatch"), "{error}");
    assert!(error.contains("strict"), "{error}");
}
