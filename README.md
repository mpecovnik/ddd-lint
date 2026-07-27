# DDD Lint

DDD Lint is a planned Rust-based Python linter for opinionated domain-driven design proxies.

It identifies syntax and type structures that deserve domain-model review without claiming to prove that a model is wrong. Rules are individually selectable, grouped into families, and suppressible through narrow design exceptions that require written reasons.

> Status: v1 behavior and architecture are specified. The CLI is not implemented or published yet.

## Motivation

A broad model with many nullable fields often represents several valid states at once:

```python
from datetime import datetime
from pydantic import BaseModel


class Subscription(BaseModel):
    customer_id: str
    trial_ends_at: datetime | None
    activated_at: datetime | None
    cancelled_at: datetime | None
```

Separate variants make valid state data explicit:

```python
class TrialSubscription(BaseModel):
    customer_id: str
    trial_ends_at: datetime


class ActiveSubscription(BaseModel):
    customer_id: str
    activated_at: datetime


class CancelledSubscription(BaseModel):
    customer_id: str
    activated_at: datetime
    cancelled_at: datetime


Subscription = TrialSubscription | ActiveSubscription | CancelledSubscription
```

Once a type has finite alternatives, behavior can be checked exhaustively:

```python
def label(subscription: Subscription) -> str:
    match subscription:
        case TrialSubscription():
            return "trial"
        case ActiveSubscription():
            return "active"
        case CancelledSubscription():
            return "cancelled"
```

DDD Lint also detects hidden constructor fallbacks that make domain services choose their own collaborators instead of receiving explicit composition.

## Planned Installation

PyPI binary wheels will be the primary distribution:

```shell
uv tool install ddd-lint
```

`pipx install ddd-lint` will provide the same `ddd-lint` executable.

## Quick Start

No rules are enabled by default. Add the recommended profile to the project or uv workspace root `pyproject.toml`:

```toml
[tool.ddd-lint]
select = ["DM002", "CT", "DI001"]

[tool.ddd-lint.rules.DM002]
min-nullable-fields = 3
min-nullable-ratio = 0.30
```

Then run:

```shell
ddd-lint check .
```

Strict model construction can enable per-field checks and stale-suppression detection:

```toml
[tool.ddd-lint]
select = ["DM001", "DM002", "DM003", "CT", "DI001", "SUP001"]
```

## Rules

| Code | Rule | Purpose |
| --- | --- | --- |
| [`DM001`](docs/rules/dm001-nullable-model-field.md) | Nullable model field | Reject any Pydantic field explicitly admitting `None`. |
| [`DM002`](docs/rules/dm002-nullable-heavy-model.md) | Nullable-heavy model | Detect configured concentrations of nullable fields. |
| [`DM003`](docs/rules/dm003-defaulted-model-field.md) | Defaulted model field | Require explicit values at Pydantic model construction. |
| [`CT001`](docs/rules/ct001-non-exhaustive-match.md) | Non-exhaustive match | Require explicit coverage of closed-type members. |
| [`CT002`](docs/rules/ct002-catch-all-on-closed-type.md) | Catch-all on closed type | Keep new members visible by rejecting wildcard cases. |
| [`CT003`](docs/rules/ct003-conditional-closed-type-dispatch.md) | Conditional closed-type dispatch | Prefer exhaustive match over member-selecting conditionals. |
| [`CT004`](docs/rules/ct004-discriminator-match.md) | Discriminator match | Match class unions through member class patterns. |
| [`DI001`](docs/rules/di001-hidden-constructor-fallback.md) | Hidden constructor fallback | Require explicit collaborator composition. |
| [`SUP001`](docs/rules/sup001-unused-suppression.md) | Unused suppression | Remove stale design exceptions. |

Selectors accept full codes, family prefixes such as `DM` or `CT`, and `ALL`. Family and `ALL` selectors include future rules added under that selector.

## Design Exceptions

Legitimate violations use DDD Lint's own directive namespace. Every exception requires explicit codes and a reason:

```python
class Person(BaseModel):
    middle_name: str | None  # ddd-lint: ignore[DM001] - A person may have no middle name
```

A directive above a function or class covers its subtree:

```python
# ddd-lint: ignore[CT003] - Adapter mirrors a third-party callback protocol
def dispatch_external_message(message: Message) -> None:
    ...
```

File-level exceptions remain explicit:

```python
# ddd-lint: file-ignore[DM001, DM003] - Wire schema mirrors partner API
```

Configuration-level path exceptions also require reasons:

```toml
[[tool.ddd-lint.per-file-ignores]]
pattern = "tests/fixtures/**"
rules = ["DM001", "DM003"]
reason = "Fixtures intentionally represent malformed external payloads"
```

Blanket ignores are unsupported.

## Project Analysis

DDD Lint will use a project-wide declared-type graph rather than a full type checker. It resolves first-party imports, aliases, annotations, class fields, inheritance, attributes, simple assignments, and annotated returns. Unknown dynamic types are skipped instead of guessed.

Normal projects use `.` and `src` as default import roots. uv workspaces are detected from `[tool.uv.workspace]`, analyzed as one cross-package graph, and governed by the root `[tool.ddd-lint]` policy.

V1 targets Python 3.10+ and Pydantic 2. Pydantic dataclasses, `RootModel`, notebooks, persistent cache, editor integration, and autofix are deferred.

## Documentation

- [V1 design](docs/design/v1.md)
- [Rule reference](docs/rules/README.md)
- [Project language](CONTEXT.md)
- [ADR 0001: Standalone CLI with Ruff parser](docs/adr/0001-standalone-cli-with-ruff-parser.md)
- [ADR 0002: Declared project graph](docs/adr/0002-declared-project-graph.md)
- [ADR 0003: Reasoned design exceptions](docs/adr/0003-reasoned-design-exceptions.md)

## Development Prerequisites

Implementation will require Rust 1.95 or newer, Cargo, Python 3.10 or newer, and maturin. The parser dependency is exact-pinned and isolated behind an internal adapter because Ruff component crates expose unstable `0.0.x` APIs.

## License

MIT
