use std::{fs, process::Command};

fn check(source: &str, args: &[&str]) -> (i32, Vec<serde_json::Value>) {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("models.py"), source).unwrap();
    fs::write(
        directory.path().join("pyproject.toml"),
        "[tool.ddd-lint]\nselect = [\"ALL\"]\nignore = [\"DM003\"]\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ddd-lint"))
        .current_dir(directory.path())
        .args(["check", "--output-format", "json"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    (
        output.status.code().unwrap(),
        serde_json::from_slice(&output.stdout).unwrap(),
    )
}

#[test]
fn defaults_are_classified_without_exempting_zero_policies() {
    let (_, findings) = check(
        r#"from typing import Literal, Annotated
from pydantic import BaseModel, Field
class Model(BaseModel):
    kind: Literal["ready"] = "ready"
    tag: Annotated[Literal["tag"], Field(default="tag")]
    wrong: Literal["ready"] = "wrong"
    note: str | None = None
    retry_count: int = 0
    failed_count: int = 0
    severity: str = "warning"
    generated: str = Field(default_factory=str)
"#,
        &["--raw"],
    );
    let defaults = findings
        .iter()
        .filter(|f| {
            matches!(
                f["code"].as_str(),
                Some("DM005" | "DM006" | "DM007" | "DM011")
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(defaults.len(), 6, "{findings:?}");
    assert_eq!(defaults.iter().filter(|f| f["code"] == "DM006").count(), 2);
    assert!(
        !defaults
            .iter()
            .any(|f| f["fields"] == serde_json::json!(["kind"])
                || f["fields"] == serde_json::json!(["tag"]))
    );
}

#[test]
fn immutable_values_are_not_collaborators() {
    let (_, findings) = check(
        r#"from pydantic import RootModel, BaseModel, Field
class LocalPort(RootModel[int], frozen=True):
    root: int = Field(ge=1)
class Mutable(RootModel[list[int]], frozen=True): pass
class Service:
    def __init__(self, port=LocalPort(15432), flags=frozenset(), mutable=Mutable([]), client=Client()): pass
"#,
        &["--profile", "recommended"],
    );
    let di = findings
        .iter()
        .filter(|f| f["code"] == "DI001")
        .collect::<Vec<_>>();
    assert_eq!(di.len(), 2, "{findings:?}");
    assert!(
        di.iter()
            .any(|f| f["message"].as_str().unwrap().contains("`mutable`"))
    );
}

#[test]
fn validators_supply_specific_findings_and_deduplicate_audits() {
    let source = r#"from pydantic import BaseModel, model_validator
from typing import Literal
class Outcome(BaseModel):
    value: str | None
    error: str | None
    @model_validator(mode="after")
    def validate(self):
        if (self.value is None) == (self.error is None):
            raise ValueError("exactly one")
        return self
class Metadata(BaseModel):
    actor: str | None
    time: str | None
    @model_validator(mode="after")
    def validate(self):
        if (self.actor is None) != (self.time is None):
            raise ValueError("both or neither")
        return self
class Job(BaseModel):
    status: Literal["pending", "done"]
    result: str | None
    @model_validator(mode="after")
    def validate(self):
        if self.status == "done" and self.result is None:
            raise ValueError("result required")
        return self
"#;
    let (_, findings) = check(source, &[]);
    assert_eq!(findings.len(), 3, "{findings:?}");
    for code in ["DM008", "DM009", "DM010"] {
        let finding = findings.iter().find(|f| f["code"] == code).unwrap();
        assert_eq!(finding["confidence"], "high");
        assert!(!finding["related"].as_array().unwrap().is_empty());
    }
    let (_, raw) = check(source, &["--raw"]);
    assert_eq!(raw.len(), 8);
}

#[test]
fn coexistence_is_nonblocking_review() {
    let (status, findings) = check(
        "from pydantic import BaseModel\nfrom typing import Literal\nclass Job(BaseModel):\n    status: Literal['pending', 'done']\n    result: str | None\n",
        &["--profile", "recommended"],
    );
    assert_eq!(status, 0);
    assert_eq!(findings[0]["confidence"], "low");
    assert_eq!(findings[0]["severity"], "review");
}

#[test]
fn strict_audit_still_blocks_after_deduplication() {
    let source = "from pydantic import BaseModel\nfrom typing import Literal\nclass Job(BaseModel):\n    status: Literal['pending', 'done']\n    result: str | None\n";
    let (status, findings) = check(source, &["--profile", "strict-construction"]);
    assert_eq!(status, 1);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0]["related"][0]["rule"], "DM001");
}

#[test]
fn mutable_builtin_defaults_are_separate_from_collaborators() {
    let (_, findings) = check(
        "class Service:\n    def __init__(self, values=list(), cache=None):\n        self.cache = cache if cache is not None else dict()\n",
        &[],
    );
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0]["code"], "DI002");
}

#[test]
fn unrelated_methods_and_finite_configuration_values_are_not_invariants() {
    let (_, findings) = check(
        r#"from pydantic import BaseModel
from typing import Literal
class Settings(BaseModel):
    architecture: Literal["arm", "x86"]
    value: str | None
    error: str | None
    def check(self):
        if (self.value is None) == (self.error is None):
            raise ValueError("not an automatic validator")
"#,
        &["--profile", "recommended"],
    );
    assert!(findings.is_empty(), "{findings:?}");
}
