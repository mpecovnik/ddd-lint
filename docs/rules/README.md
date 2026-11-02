# Rule Reference

No rule is enabled by default. Select a full code, a family prefix, or `ALL` in `[tool.ddd-lint]`.

Pydantic model rules inspect `BaseModel` subclasses but exclude `RootModel` and its descendants.

## Domain Model

| Code | Name | Summary |
| --- | --- | --- |
| [`DM001`](dm001-nullable-model-field.md) | `nullable-model-field` | Require every Pydantic model field to reject `None`. |
| [`DM002`](dm002-nullable-heavy-model.md) | `nullable-heavy-model` | Detect models with a configured concentration of nullable fields. |
| [`DM003`](dm003-defaulted-model-field.md) | `defaulted-model-field` | Require callers to supply every Pydantic model field explicitly. |
| [`DM004`](dm004-nullable-event-metadata-pair.md) | `nullable-event-metadata-pair` | Group nullable `<prefix>_by` / `<prefix>_at` pairs into optional metadata values. |

## Closed Type

| Code | Name | Summary |
| --- | --- | --- |
| [`CT001`](ct001-non-exhaustive-match.md) | `non-exhaustive-match` | Require explicit coverage of every closed-type member. |
| [`CT002`](ct002-catch-all-on-closed-type.md) | `catch-all-on-closed-type` | Reject wildcard or capture fallbacks on closed types. |
| [`CT003`](ct003-conditional-closed-type-dispatch.md) | `conditional-closed-type-dispatch` | Replace multi-member conditional dispatch with pattern matching; optionally enforce strict member-test policy. |
| [`CT004`](ct004-discriminator-match.md) | `discriminator-match` | Match a class union by member class, not discriminator value. |

## Dependency Injection

| Code | Name | Summary |
| --- | --- | --- |
| [`DI001`](di001-hidden-constructor-fallback.md) | `hidden-constructor-fallback` | Require composition to supply call-created collaborators explicitly. |

## Suppression Hygiene

| Code | Name | Summary |
| --- | --- | --- |
| [`SUP001`](sup001-unused-suppression.md) | `unused-suppression` | Remove design exceptions that suppress no enabled diagnostic. |

## Recommended Profiles

Practical onboarding:

```toml
[tool.ddd-lint]
select = ["DM004", "CT", "DI001"]
```

Onboarding uses DM004's focused event-metadata check rather than DM002's broad nullable-concentration heuristic. Add DM002 explicitly when that broader review is wanted. CT003 uses its default `dispatch` mode: multi-branch variant dispatch is reported, while single predicates and type guards remain allowed.

Strict model construction and conditional-member policy:

```toml
[tool.ddd-lint]
select = ["DM001", "DM002", "DM003", "DM004", "CT", "DI001", "SUP001"]

[tool.ddd-lint.rules.CT003]
mode = "strict"
```

Family selectors include future rules added to that family. Select exact codes when upgrades must not expand policy automatically.

CT003 previously applied strict behavior whenever selected. Set `mode = "strict"` to retain that policy; `CT` and `ALL` now use `dispatch` unless configured otherwise.
