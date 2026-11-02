# DM002: Nullable-Heavy Model

Rule name: `nullable-heavy-model`

Origin: opinionated DDD proxy.

Default: disabled.

## Summary

Detect a Pydantic `BaseModel` whose declared fields meet both a nullable count and nullable ratio threshold. A finding invites review of field relationships; it does not establish a domain-model defect.

Recommended onboarding uses [DM004](dm004-nullable-event-metadata-pair.md) for focused nullable actor/time pairs. Enable DM002 additionally when broader concentration-based review is wanted.

## Why

Clusters of nullable fields can indicate one broad schema representing several lifecycle states. If different field combinations are valid in different states, a class union can represent those states directly.

The same count can also describe independent optional facts, incomplete observations, or a read-model projection. DM002 counts declared nullable fields; it does not infer correlations, interpret validators, or establish a reachable application failure. Its diagnostic help is deliberately a review prompt:

> Review whether nullable fields encode correlated states; independent absence may be intentional.

Before choosing a rewrite, establish what absence means and which combinations the domain permits:

- **State-dependent data:** success evidence and failure details belong to different outcomes. Consider explicit state variants.
- **Fields present or absent together:** a revision ID, selecting actor, and selection timestamp may form one optional value object.
- **Independent absence:** filters, partial addresses, unknown metrics, and separately acquired evidence can legitimately vary independently. Preserve those combinations.

Improving one correlated group does not require splitting the entire model. Review producers, consumers, existing validation, and behavior tests to identify the actual relationship.

## Configuration

Defaults:

```toml
[tool.ddd-lint.rules.DM002]
min-nullable-fields = 3
min-nullable-ratio = 0.30
```

Both gates must be met. The denominator contains Pydantic fields declared in the current class body. Inherited fields are checked only where declared.

`min-nullable-fields` must be at least `1`. `min-nullable-ratio` must be greater than `0.0` and no greater than `1.0`. `RootModel` and its descendants are excluded.

## State-Dependent Fields

This model has three of six fields nullable and meets the default thresholds:

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

If the domain has distinct trial, active, and cancelled states with the field requirements shown below, prefer variants that contain only state-valid data:

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

## Fields That Belong Together

If a selection's revision, actor, and time must all be present or all absent, group that relationship rather than creating variants for every other field in the containing model:

```python
class Selection(BaseModel):
    revision_id: str
    selected_by: str
    selected_at: datetime


class StudyView(BaseModel):
    study_id: str
    selection: Selection | None
```

The all-or-none requirement must come from the domain contract, not from the nullable count.

## Independent Optional Fields

Each filter below independently narrows a query. `None` means no restriction on that dimension, not a lifecycle state:

```python
class SearchFilters(BaseModel):
    city: str | None
    postal_code: str | None
    owner_id: str | None
```

This still meets DM002's thresholds. A union enumerating combinations would obscure the intended independence. Column mappings, partial postal addresses, and geographic hints can have the same property. Document a reasoned exception when this representation is intentional.

## Partial Evidence and Independent Outcomes

An observation can retain useful evidence even when another source fails:

```python
class SizingResult(BaseModel):
    extent: tuple[float, float, float, float] | None
    extent_error: str | None
    source_error: str | None


result = SizingResult(
    extent=(14.4, 46.0, 14.6, 46.2),
    extent_error=None,
    source_error="Uploaded source failed; extent came from another source",
)
```

A whole-result success/failure union that forbids an extent alongside a source error would remove supported behavior. If the extent/error pair is state-dependent, consider grouping just that pair into extent outcome variants, including an unmeasured state if the domain permits one. Keep source failure as an independent dimension.

Sparse external observations and incomplete metadata can likewise be valuable evidence rather than invalid domain states. A validator may already enforce the relevant relationship; inspect that contract before proposing a change.

## Metrics and Read-Model Projections

```python
class OperationsView(BaseModel):
    queued_count: int | None
    running_count: int | None
    last_failure: str | None
```

If counts come from independent observations, `None` can mean unknown rather than zero. A retained failure can coexist with either known count. UI and read-model projections often combine such independent facts; reducing them to one success/failure state loses information.

Class names, transport roles, and the presence of validators do not automatically exempt a model. The same contexts can contain genuinely correlated state fields. DM002 remains a threshold-based review prompt, with explicit exceptions for accepted representations.

## Design Exception

When a nullable-heavy model intentionally mirrors an external sparse record:

```python
# ddd-lint: ignore[DM002] - Partner API defines this sparse wire representation
class PartnerRecord(BaseModel):
    ...
```

For independent facts, make the reason equally specific, for example `# ddd-lint: ignore[DM002] - Query filters independently constrain different dimensions` above `SearchFilters`.

`DM002` reports at class level. Field-level `DM001` exceptions do not suppress the aggregate `DM002` finding.
