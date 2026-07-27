# CT002: Catch-All on Closed Type

Rule name: `catch-all-on-closed-type`

Origin: closed-type modeling rule; DDD-adjacent.

Default: disabled.

## Summary

When enabled, reject wildcard and capture fallback cases when matching a closed type.

## Why

A catch-all makes a match total today but hides future members from ordinary control flow. Explicit cases preserve change detection. `CT001`, when enabled separately, ignores wildcard coverage and reports members missing from explicit patterns.

DDD Lint provides `CT001` as its exhaustiveness check, so an `assert_never` wildcard pattern is unnecessary when both rules are selected.

## Example

Problematic:

```python
def label(status: Status) -> str:
    match status:
        case Status.OPEN:
            return "open"
        case _:
            return "other"
```

Prefer explicit members:

```python
def label(status: Status) -> str:
    match status:
        case Status.OPEN:
            return "open"
        case Status.CLOSED:
            return "closed"
```

`CT002` also reports a redundant wildcard after all known members:

```python
match status:
    case Status.OPEN:
        open_order()
    case Status.CLOSED:
        close_order()
    case _:
        unreachable()
```

When a wildcard hides missing explicit members, `CT001` may report the missing members and `CT002` reports the wildcard.

## Design Exception

If runtime values can violate trusted typing and defensive fallback is required:

```python
match status:
    case Status.OPEN:
        open_order()
    case Status.CLOSED:
        close_order()
    case _:  # ddd-lint: ignore[CT002] - Deserialized legacy values can bypass enum validation
        quarantine(status)
```
