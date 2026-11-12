use std::{fs, process::Command};

#[test]
fn singleton_enum_defaults_match_member_identity_across_imports_and_field_forms() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("states.py"),
        r#"from enum import Enum, IntEnum, StrEnum
from typing import Literal
class State(Enum):
    READY = "ready"
    WAITING = "waiting"
class Other(Enum):
    READY = "ready"
class Count(IntEnum):
    ZERO = 0
class Text(StrEnum):
    READY = "ready"
class Constants:
    READY = "ready"
Ready = Literal[State.READY]
"#,
    )
    .unwrap();
    fs::write(
        directory.path().join("exports.py"),
        "from states import State as ExportedState\n",
    )
    .unwrap();
    fs::write(
        directory.path().join("models.py"),
        r#"from typing import Annotated, Literal
from pydantic import BaseModel, Field
from states import State, Other, Count, Text, Constants, Ready
from exports import ExportedState as Alias
import states

class Model(BaseModel):
    direct: Literal[State.READY] = State.READY
    positional: Literal[State.READY] = Field(State.READY)
    keyword: Literal[State.READY] = Field(default=Alias.READY)
    annotated: Annotated[Literal[State.READY], Field(default=states.State.READY)]
    alias: Ready = Alias.READY
    reexport: Literal[Alias.READY] = State.READY
    integer: Literal[Count.ZERO] = Count.ZERO
    string_enum: Literal[Text.READY] = Text.READY
    plain: Literal["ready"] = "ready"
    wrong: Literal[State.READY] = State.WAITING
    other_enum: Literal[State.READY] = Other.READY
    raw_string: Literal[Text.READY] = "ready"
    raw_integer: Literal[Count.ZERO] = 0
    multiple: Literal[State.READY, State.WAITING] = State.READY
    ordinary: Literal[Constants.READY] = Constants.READY
    unknown: Literal[State.MISSING] = State.MISSING
    factory: Literal[State.READY] = Field(default_factory=lambda: State.READY)
"#,
    )
    .unwrap();

    for rule in ["DM003", "DM011"] {
        fs::write(
            directory.path().join("pyproject.toml"),
            format!("[tool.ddd-lint]\nselect = [\"{rule}\"]\n"),
        )
        .unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ddd-lint"))
            .current_dir(directory.path())
            .args(["check", "--output-format", "json"])
            .output()
            .unwrap();
        assert!(
            output.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let findings: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
        let fields = findings
            .iter()
            .map(|finding| finding["fields"][0].as_str().unwrap())
            .collect::<Vec<_>>();
        let mut expected = vec![
            "wrong",
            "other_enum",
            "raw_string",
            "raw_integer",
            "multiple",
            "ordinary",
            "unknown",
        ];
        if rule == "DM003" {
            expected.push("factory");
        } else {
            expected.retain(|field| *field != "raw_integer");
        }
        assert_eq!(fields, expected, "{rule}: {findings:?}");
        assert!(findings.iter().all(|finding| finding["code"] == rule));
    }
}
