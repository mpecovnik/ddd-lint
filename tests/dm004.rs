use std::fs;
use std::process::{Command, Output};

fn check(source: &str, config: &str) -> Output {
    let directory = tempfile::tempdir().expect("temporary project");
    fs::write(
        directory.path().join("base.py"),
        "from datetime import datetime\nfrom pydantic import BaseModel\nclass Model(BaseModel): pass\nActor = str\nTime = datetime\nMaybeTime = datetime | None\n",
    )
    .expect("supporting declarations");
    fs::write(directory.path().join("models.py"), source).expect("model fixture");
    fs::write(directory.path().join("pyproject.toml"), config).expect("configuration");
    Command::new(env!("CARGO_BIN_EXE_ddd-lint"))
        .current_dir(directory.path())
        .args(["check", "models.py", "--output-format", "json"])
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
fn dm004_reports_pairs_below_dm002_threshold_through_aliases_and_inheritance() {
    // Two nullable actor/time fields suffice, independently of DM002's three-field
    // threshold. Resolve first-party aliases, Optional and Annotated declarations;
    // report once at the earlier field, and do not repeat inherited pairs.
    let source = r#"from typing import Annotated, Optional
from pydantic import Field
from base import Model, Actor, Time, MaybeTime
class Selection(Model):
    selected_by: Actor | None = None
    selected_at: Time | None = None
class Saved(Model):
    saved_at: MaybeTime
    saved_by: "Actor | None"
class InputSelection(Model):
    input_selected_by: Annotated[Optional[Actor], Field(description="Actor")]
    input_selected_at: Optional[Time]
class Child(Selection):
    title: str
"#;
    let output = check(source, "[tool.ddd-lint]\nselect = [\"DM004\", \"DM002\"]\n");
    let findings = diagnostics(&output);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(findings.len(), 3, "{findings:?}");
    for (finding, (prefix, line)) in
        findings
            .iter()
            .zip([("selected", 5), ("saved", 8), ("input_selected", 11)])
    {
        assert_eq!(finding["code"], "DM004");
        assert_eq!(finding["range"]["start"]["line"], line);
        assert_eq!(
            finding["message"],
            format!(
                "nullable event metadata `{prefix}_by` and `{prefix}_at` share prefix `{prefix}`"
            )
        );
    }
}

#[test]
fn dm004_reports_each_event_pair_without_claiming_unrelated_fields() {
    // Selection and archival metadata are separate pairs. A nullable revision ID
    // does not become part of either lexical grouping; DM002 remains independent.
    let source = r#"from base import Model, Actor, Time
class History(Model):
    revision_id: str | None
    selected_by: Actor | None
    selected_at: Time | None
    archived_at: Time | None
    archived_by: Actor | None
"#;
    let output = check(source, "[tool.ddd-lint]\nselect = [\"DM004\", \"DM002\"]\n");
    let findings = diagnostics(&output);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(findings.len(), 3, "{findings:?}");
    assert_eq!(findings[0]["code"], "DM002");
    for (finding, prefix) in findings[1..].iter().zip(["selected", "archived"]) {
        assert_eq!(finding["code"], "DM004");
        let message = finding["message"].as_str().expect("message");
        assert!(message.contains(prefix), "{message}");
        assert!(!message.contains("revision_id"), "{message}");
    }
}

#[test]
fn dm004_skips_other_groups_short_prefixes_and_non_nullable_or_non_model_fields() {
    // Only matching _by/_at pairs with a five-character prefix and two top-level
    // nullable model fields qualify. Independent data, required or unknown types,
    // nested nullability, excluded fields/models, and split declarations stay clean.
    let source = r#"from typing import Any, ClassVar
from pydantic import RootModel
from vendor import UnknownModel, UnknownTime
from base import Model, Actor, Time
class Independent(Model):
    current_temperature: float | None
    current_pressure: float | None
    geometry_column: str | None
    geometry_type: str | None
class Short(Model):
    done_by: Actor | None
    done_at: Time | None
class Mismatched(Model):
    created_by: Actor | None
    updated_at: Time | None
class RequiredActor(Model):
    selected_by: Actor
    selected_at: Time | None
class RequiredTime(Model):
    selected_by: Actor | None
    selected_at: Time
class DefaultOnly(Model):
    selected_by: Actor = None
    selected_at: Time = None
class Nested(Model):
    selected_by: list[Actor | None]
    selected_at: Time | None
class Unknown(Model):
    selected_by: Actor | None
    selected_at: UnknownTime
class Opaque(Model):
    selected_by: Any
    selected_at: object
class ExcludedFields(Model):
    _selected_by: Actor | None
    _selected_at: Time | None
    created_by: ClassVar[Actor | None]
    created_at: ClassVar[Time | None]
class Plain:
    selected_by: Actor | None
    selected_at: Time | None
class External(UnknownModel):
    selected_by: Actor | None
    selected_at: Time | None
class Root(RootModel[str]):
    selected_by: Actor | None
    selected_at: Time | None
class RootChild(Root):
    created_by: Actor | None
    created_at: Time | None
class Parent(Model):
    selected_by: Actor | None
class Child(Parent):
    selected_at: Time | None
"#;
    let output = check(source, "[tool.ddd-lint]\nselect = [\"DM004\"]\n");
    assert_eq!(output.status.code(), Some(0), "{:?}", diagnostics(&output));
    assert!(diagnostics(&output).is_empty());
}

#[test]
fn dm004_accepts_pair_and_class_exceptions_and_reports_stale_pair_exceptions() {
    // A reason at the first field or class suppresses the pair. If one field is
    // required, the pair no longer qualifies and SUP001 identifies the stale reason.
    // Family selection includes DM004 while explicit ignores remove broader rules.
    let source = r#"from base import Model, Actor, Time
class SystemEvent(Model):
    created_at: Time | None  # ddd-lint: ignore[DM004] - System events have no actor
    created_by: Actor | None
# ddd-lint: ignore[DM004] - Wire format preserves independently missing attribution
class ImportedEvent(Model):
    imported_by: Actor | None
    imported_at: Time | None
class RequiredEvent(Model):
    # ddd-lint: ignore[DM004] - Legacy optional metadata
    selected_by: Actor | None
    selected_at: Time
"#;
    let output = check(
        source,
        "[tool.ddd-lint]\nselect = [\"DM\", \"SUP001\"]\nignore = [\"DM001\", \"DM002\", \"DM003\"]\n",
    );
    let findings = diagnostics(&output);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0]["code"], "SUP001");
    assert_eq!(findings[0]["range"]["start"]["line"], 10);
}

#[test]
fn recommended_profile_reports_event_pair_but_allows_independent_optional_data() {
    // The new onboarding profile catches a two-field metadata pair while leaving
    // independent nullable filters alone. Nothing is enabled without selection.
    let source = r#"from base import Model, Actor, Time
class Selection(Model):
    selected_by: Actor | None
    selected_at: Time | None
class Filters(Model):
    city: str | None
    postal_code: str | None
    owner_id: str | None
"#;
    let output = check(
        source,
        "[tool.ddd-lint]\nselect = [\"DM004\", \"CT\", \"DI001\"]\n",
    );
    let findings = diagnostics(&output);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0]["code"], "DM004");

    let disabled = check(source, "[tool.ddd-lint]\n");
    assert_eq!(disabled.status.code(), Some(0));
    assert!(diagnostics(&disabled).is_empty());
}
