# CT001: Non-Exhaustive Match

Rule name: `non-exhaustive-match`

Origin: closed-type modeling rule; DDD-adjacent.

Default: disabled.

## Summary

Require `match` statements over closed types to cover every member explicitly.

## Why

Closed types make valid alternatives enumerable. Explicit coverage turns a newly added state or value into a diagnostic at every affected dispatch site. Missing behavior remains visible instead of falling through silently.

V1 checks class unions, `Literal` alternatives, and non-flag enums. It reports only when the subject type and finite member set can be resolved.

## Example: Class Union

Problematic:

```python
class Draft:
    pass


class Published:
    pass


class Archived:
    pass


Document = Draft | Published | Archived


def label(document: Document) -> str:
    match document:
        case Draft():
            return "draft"
        case Published():
            return "published"
```

Prefer complete member coverage:

```python
def label(document: Document) -> str:
    match document:
        case Draft():
            return "draft"
        case Published():
            return "published"
        case Archived():
            return "archived"
```

## Example: Enum

```python
from enum import Enum


class Status(Enum):
    OPEN = "open"
    CLOSED = "closed"


def label(status: Status) -> str:
    match status:
        case Status.OPEN:
            return "open"
        case Status.CLOSED:
            return "closed"
```

## Coverage Rules

- OR patterns cover each named member.
- `as` patterns preserve coverage from their inner pattern.
- Guarded cases are partial and do not count without an unguarded case for that member.
- Base-class, mapping, sequence, wildcard, and capture patterns do not count as explicit member coverage.
- `A | B | None` requires `case None`.
- Optional-only `T | None` is outside closed-type analysis.

Class members must be pairwise non-overlapping in the known hierarchy. Literal members must not compare equal under Python pattern semantics. DDD Lint skips ambiguous unions such as a known `Base | Child` or `Literal[1, True]` rather than reporting unsound exhaustiveness.

## Design Exception

Suppress the match statement only when partial handling is deliberate and externally guaranteed:

```python
# ddd-lint: ignore[CT001] - Caller filters archived documents before dispatch
match document:
    case Draft():
        prepare(document)
    case Published():
        publish(document)
```
