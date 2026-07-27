# CT003: Conditional Closed-Type Dispatch

Rule name: `conditional-closed-type-dispatch`

Origin: closed-type modeling rule; DDD-adjacent.

Default: disabled.

## Summary

Reject direct closed-member tests in `if`, `elif`, and ternary expressions. Use exhaustive pattern matching instead.

## Why

Conditional dispatch makes omitted alternatives easy to overlook. Pattern matching gives the linter one structured dispatch site whose coverage can be checked against the closed type.

This rule is intentionally strict: even a one-off early-return member test triggers. Assertions, loops, comprehensions, and match guards remain outside v1.

## Recognized Tests

- `isinstance(value, Member)`
- `type(value) is Member`
- enum and literal equality or identity comparisons
- recognized discriminator-field equality
- negated forms and reversed operands

General helper predicates, membership tests, and arbitrary boolean/set reasoning are not resolved in v1.

## Example: Class Union

Problematic:

```python
def label(document: Document) -> str:
    if isinstance(document, Draft):
        return "draft"
    elif isinstance(document, Published):
        return "published"
    return "archived"
```

Prefer:

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

Problematic:

```python
message = "open" if status == Status.OPEN else "closed"
```

Prefer:

```python
match status:
    case Status.OPEN:
        message = "open"
    case Status.CLOSED:
        message = "closed"
```

## Design Exception

Adapter code may need to mirror a third-party conditional protocol:

```python
# ddd-lint: ignore[CT003] - Vendor callback contract requires this fast-path branch
if event.kind == "heartbeat":
    acknowledge(event)
```
