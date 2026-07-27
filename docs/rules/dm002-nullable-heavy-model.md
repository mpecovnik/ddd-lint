# DM002: Nullable-Heavy Model

Rule name: `nullable-heavy-model`

Origin: opinionated DDD proxy.

Default: disabled.

## Summary

Detect a Pydantic `BaseModel` whose declared fields meet both a nullable count and nullable ratio threshold.

## Why

Clusters of nullable fields often indicate one broad schema representing several lifecycle states. Different field combinations become valid at different times, while the type admits many combinations that the domain does not.

A class union can represent each valid state directly. This rule is a smell heuristic: nullable-heavy transport schemas and sparse external records can be intentional.

## Configuration

Defaults:

```toml
[tool.ddd-lint.rules.DM002]
min-nullable-fields = 3
min-nullable-ratio = 0.30
```

Both gates must be met. The denominator contains Pydantic fields declared in the current class body. Inherited fields are checked only where declared.

`min-nullable-fields` must be at least `1`. `min-nullable-ratio` must be greater than `0.0` and no greater than `1.0`. `RootModel` and its descendants are excluded.

## Example

Problematic with three of six fields nullable:

```python
from datetime import datetime
from pydantic import BaseModel


class Subscription(BaseModel):
    customer_id: str
    plan_id: str
    trial_ends_at: datetime | None
    activated_at: datetime | None
    cancelled_at: datetime | None
    version: int
```

Prefer variants that contain only state-valid data:

```python
class TrialSubscription(BaseModel):
    customer_id: str
    plan_id: str
    trial_ends_at: datetime
    version: int


class ActiveSubscription(BaseModel):
    customer_id: str
    plan_id: str
    activated_at: datetime
    version: int


class CancelledSubscription(BaseModel):
    customer_id: str
    plan_id: str
    activated_at: datetime
    cancelled_at: datetime
    version: int


Subscription = TrialSubscription | ActiveSubscription | CancelledSubscription
```

## Design Exception

When a nullable-heavy model intentionally mirrors an external sparse record:

```python
# ddd-lint: ignore[DM002] - Partner API defines this sparse wire representation
class PartnerRecord(BaseModel):
    ...
```

`DM002` reports at class level. Field-level `DM001` exceptions do not suppress the aggregate `DM002` finding.
