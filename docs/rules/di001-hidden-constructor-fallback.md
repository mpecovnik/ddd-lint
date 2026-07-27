# DI001: Hidden Constructor Fallback

Rule name: `hidden-constructor-fallback`

Origin: dependency-injection and composition proxy; DDD-adjacent.

Default: disabled.

## Summary

Reject constructor policies that replace an omitted argument with a call-created collaborator.

## Why

Hidden fallbacks make a class choose its own composition policy. Call sites no longer reveal whether production, no-op, in-memory, system-clock, or other behavior was selected. Explicit injection keeps composition at the composition root and makes policy visible in tests and production wiring.

DDD does not prohibit a Null Object. `NoopRepository()` can be valid when the composition root injects it deliberately. The hidden choice is the concern.

## What It Checks

V1 treats `__init__` as the constructor boundary and detects its parameters whose omission selects a call expression, including direct call defaults and common body fallbacks:

```python
def __init__(self, repository: Repository = NoopRepository()): ...
def __init__(self, repository: Repository | None = None):
    self.repository = repository or NoopRepository()
```

A direct call default runs once when Python defines the function and reuses that object for every omitted argument. A body fallback runs per constructor call. An `argument or Fallback()` expression also replaces explicitly supplied falsy values, while an `if argument is None` fallback only replaces `None`. All forms hide composition and trigger when recognized.

Equivalent conditional-expression and `if parameter is None` assignments also trigger when resolved syntactically. Plain scalar or pre-existing constant defaults remain outside `DI001`.

`__new__`, classmethod factories, and ordinary functions are outside `DI001` in v1.

## Example

Problematic:

```python
class CheckoutService:
    def __init__(self, repository: OrderRepository | None = None):
        self.repository = repository or InMemoryOrderRepository()
```

Prefer required injection:

```python
class CheckoutService:
    def __init__(self, repository: OrderRepository):
        self.repository = repository


service = CheckoutService(repository=PostgresOrderRepository(connection))
```

Tests can inject the no-op or in-memory implementation explicitly:

```python
service = CheckoutService(repository=InMemoryOrderRepository())
```

## Design Exception

When a standalone utility intentionally owns composition:

```python
class LocalPreview:
    def __init__(self, clock: Clock | None = None):
        self.clock = clock or SystemClock()  # ddd-lint: ignore[DI001] - Preview is a self-contained process boundary
```
