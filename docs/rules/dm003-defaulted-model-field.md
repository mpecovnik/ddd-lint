# DM003: Defaulted Model Field

Rule name: `defaulted-model-field`

Origin: opinionated DDD proxy.

Default: disabled.

## Summary

Require callers to provide every declared Pydantic model field explicitly.

## Why

Defaults can hide a domain decision at construction time. Call sites no longer show whether the caller chose a value or silently accepted model policy. Explicit values make creation flow visible and make factories natural homes for named domain decisions.

DDD does not prohibit defaults. A universal domain default can be valid. This rule enforces an explicit-construction convention, not a DDD law.

## What It Checks

`DM003` reports non-required Pydantic field forms except matching single-value Literal tags, such as `kind: Literal["ready"] = "ready"`:

Matching enum-member tags also qualify: `state: Literal[State.READY] = State.READY`. Imported aliases and re-exports resolve to the same member identity. A different enum member, an underlying scalar value, or a multi-value Literal does not qualify.

```python
status: Status = Status.DRAFT
tags: list[str] = []
clock: Clock = system_clock
identifier: Identifier = Identifier.new()
tags: list[str] = Field(default_factory=list)
status: Status = Field(default=Status.DRAFT)
priority: Annotated[int, Field(default=0)]
```

Any assigned expression that makes the field omittable counts, not only Python literal expressions. `Field(default=...)` and `Field(default_factory=...)` also count when supplied through `Annotated` without an assignment. Required `Field(...)` declarations and metadata-only `Annotated` declarations do not trigger. `RootModel` and its descendants are excluded.

## Example

For classification rather than blanket construction policy, use opt-in [DM005, DM006, DM007, and DM011](defaults-and-relationships.md). Recommended and review profiles do not enable DM003. When DM003 and narrower default rules are selected together, DM003 takes precedence.

Problematic:

```python
from pydantic import BaseModel, Field


class Order(BaseModel):
    lines: list[str] = Field(default_factory=list)
```

Require explicit construction:

```python
class Order(BaseModel):
    lines: list[str]


order = Order(lines=[])
```

Prefer a named factory when the value represents a domain decision:

```python
class Order(BaseModel):
    lines: list[str]

    @classmethod
    def empty_draft(cls) -> "Order":
        return cls(lines=[])
```

## Design Exception

When the bounded context defines a genuine universal default:

```python
class Quote(BaseModel):
    currency: Currency = Currency.EUR  # ddd-lint: ignore[DM003] - This context only trades in EUR
```
