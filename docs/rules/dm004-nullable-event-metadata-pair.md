# DM004: Nullable Event-Metadata Pair

Rule name: `nullable-event-metadata-pair`

Origin: opinionated DDD proxy.

Default: disabled. Included in the recommended onboarding profile.

## Summary

Detect a pair of nullable `<prefix>_by` and `<prefix>_at` fields declared in the same Pydantic model when the shared prefix is at least five characters long. Report once per pair and recommend one optional metadata value.

## Why

Pairs such as `selected_by` / `selected_at`, `created_by` / `created_at`, and `archived_by` / `archived_at` commonly describe one event's actor and timestamp. When these facts belong together, independent nullable fields admit partial metadata that a grouped value can rule out.

This focused naming convention is a stronger starting signal than counting all nullable fields. It establishes a recognizable metadata pair, not proof that partial metadata is invalid in every domain. System-generated events or imported records may legitimately lack an actor while retaining a timestamp; use a reasoned exception when those combinations are intentional.

## Detection Boundary

- Both fields must be declared in the current class body and have resolved top-level annotations admitting `None`. `Optional`, `Annotated`, quoted annotations, and resolvable aliases follow the shared model-field semantics.
- Match the complete, case-sensitive prefix before the exact `_by` and `_at` suffixes. Count at least five characters, excluding the suffix and its separating underscore. `saved_by` / `saved_at` qualifies; `done_by` / `done_at` does not. Multiword prefixes such as `input_selected` qualify.
- Require both fields to be nullable. Defaults are irrelevant: `str = None` alone does not establish nullable typing, and `list[str | None]` is not top-level nullable.
- Inspect Pydantic `BaseModel` subclasses, including resolved first-party subclasses. Exclude `RootModel` descendants, private attributes, `ClassVar`, and computed fields.
- Check inherited pairs only at their declaring class. A pair split across base and child declarations is outside this initial rule.

DM004 does not require a particular actor-ID or datetime type, infer arbitrary shared-prefix groups, interpret validators, or associate differently named fields with the pair. For example, `geometry_column` / `geometry_type` and independent `current_temperature` / `current_pressure` are outside its scope. `revision_id` is not automatically included in a `selected` group.

There are no rule-specific settings. The five-character boundary is fixed. DM002's nullable count and ratio thresholds do not apply: a model with only these two nullable fields can trigger DM004.

## Example

```python
from datetime import datetime
from pydantic import BaseModel


class CurrentInputSelection(BaseModel):
    revision_id: str | None
    selected_by: str | None
    selected_at: datetime | None
```

DM004 identifies `selected_by` and `selected_at`. If the domain requires the revision ID, actor, and timestamp to be present or absent together, include all three in one optional value:

```python
class Selection(BaseModel):
    revision_id: str
    selected_by: str
    selected_at: datetime


class CurrentInputSelection(BaseModel):
    selection: Selection | None
```

Determining whether the revision ID belongs in that value requires domain knowledge. If only the actor/time pair is correlated, group just that pair; preserve independent facts and valid partial evidence elsewhere in the model.

## Selection and Diagnostics

Recommended starting profile:

```toml
[tool.ddd-lint]
select = ["DM004", "CT", "DI001"]
```

Use DM002 additionally when broader nullable-concentration review is desired. DM001 rejects individual nullable fields; DM003 checks defaults. These rules remain independently selectable, and enabling several can produce several findings for the same model. `DM` and `ALL` include DM004; no rules are enabled without selection.

A finding names both fields and their shared prefix. It is anchored at the earlier field declaration, regardless of whether `_by` or `_at` appears first. Each pair produces one diagnostic and causes the normal enabled-violation exit code (`1`).

## Design Exception

Attach a pair-specific exception to the first field:

```python
class ImportedEvent(BaseModel):
    # ddd-lint: ignore[DM004] - Source retains timestamps for events with unknown actors
    created_at: datetime | None
    created_by: str | None
```

Class-level, file-level, and configured per-file exceptions also apply. Other rule codes do not suppress DM004. When enabled, SUP001 reports an exception that no longer suppresses a qualifying pair.
