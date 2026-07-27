# Rule Reference

No rule is enabled by default. Select a full code, a family prefix, or `ALL` in `[tool.ddd-lint]`.

Pydantic model rules inspect `BaseModel` subclasses but exclude `RootModel` and its descendants.

## Domain Model

| Code | Name | Summary |
| --- | --- | --- |
| [`DM001`](dm001-nullable-model-field.md) | `nullable-model-field` | Require every Pydantic model field to reject `None`. |
| [`DM002`](dm002-nullable-heavy-model.md) | `nullable-heavy-model` | Detect models with a configured concentration of nullable fields. |
| [`DM003`](dm003-defaulted-model-field.md) | `defaulted-model-field` | Require callers to supply every Pydantic model field explicitly. |

## Closed Type

| Code | Name | Summary |
| --- | --- | --- |
| [`CT001`](ct001-non-exhaustive-match.md) | `non-exhaustive-match` | Require explicit coverage of every closed-type member. |
| [`CT002`](ct002-catch-all-on-closed-type.md) | `catch-all-on-closed-type` | Reject wildcard or capture fallbacks on closed types. |
| [`CT003`](ct003-conditional-closed-type-dispatch.md) | `conditional-closed-type-dispatch` | Replace conditional member dispatch with pattern matching. |
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
select = ["DM002", "CT", "DI001"]
```

Strict model construction:

```toml
[tool.ddd-lint]
select = ["DM001", "DM002", "DM003", "CT", "DI001", "SUP001"]
```

Family selectors include future rules added to that family. Select exact codes when upgrades must not expand policy automatically.
