# CT002: Catch-All on Closed Type

Rule name: `catch-all-on-closed-type`

Origin: closed-type modeling rule; DDD-adjacent.

Default: disabled.

## Summary

When enabled, reject wildcard and capture fallback cases when matching a closed type, except recognized `assert_never` exhaustiveness sinks.

## Why

A catch-all makes a match total today but hides future members from ordinary control flow. Explicit cases preserve change detection. `CT001`, when enabled separately, ignores wildcard coverage and reports members missing from explicit patterns.

A named capture used only as the argument to `assert_never` is allowed. A wildcard calling `assert_never` on the match subject is also allowed. Both idioms preserve projects that deliberately delegate exhaustiveness checking to a type checker:

```python
from typing import assert_never

match status:
    case Status.OPEN:
        open_order()
    case Status.CLOSED:
        close_order()
    case unexpected:
        assert_never(unexpected)
```

Equivalent wildcard form:

```python
match status:
    case Status.OPEN:
        open_order()
    case Status.CLOSED:
        close_order()
    case _:
        assert_never(status)
```

For either idiom, the case must be unguarded and its body must consist solely of a call expression with one positional argument and no keyword arguments. The callee must resolve to `typing.assert_never` or `typing_extensions.assert_never`; imported aliases and qualified calls are supported. A shadowed or unrelated function with the same name does not qualify.

A named capture must pass that capture. A wildcard must pass the same name or name-rooted attribute chain as the match subject, such as `status`, `order.status`, or `envelope.order.status`. Parentheses do not affect matching. This is a syntactic reference comparison within the declared-type analysis boundary, not proof against dynamic attribute behavior or intervening side effects. Repeated calls such as `get_status()` and other expressions such as subscripts are not recognized as the same value.

Neither sink contributes explicit member coverage: `CT001`, when enabled, still reports missing members, including members added later. Ordinary captures and wildcards still trigger `CT002`.

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
