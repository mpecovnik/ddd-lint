# DM001: Nullable Model Field

Rule name: `nullable-model-field`

Origin: opinionated DDD proxy.

Default: disabled.

## Summary

Require every declared Pydantic `BaseModel` field to reject `None`.

## Why

A nullable field can merge distinct domain states into one shape. Callers must then know which combinations of present and absent values are valid. Separate model variants can make those states explicit and move invalid combinations outside the type.

`None` can also be a legitimate domain value. This rule intentionally chooses strict construction and requires a documented design exception for that case. It does not prove that every nullable field hides a state transition.

## What It Checks

`DM001` reports a field when its resolved top-level annotation admits `None`, including:

```python
value: str | None
value: Optional[str]
value: Union[str, None]
```

Required nullable fields still trigger. Defaults and omission are separate concerns. `Any` and `object` do not trigger because they do not explicitly declare nullability.

Nested `None` does not make the field itself nullable. `list[str | None]` admits null elements but rejects a null list, and `Callable[..., None]` describes a callable return rather than a nullable field.

Only fields declared in the current `BaseModel` class are checked. `RootModel` descendants, `ClassVar`, private attributes, computed fields, and inherited fields are excluded at that class.

## Example

Problematic:

```python
from pydantic import BaseModel


class Shipment(BaseModel):
    tracking_number: str | None
```

Prefer explicit variants when absence represents state:

```python
from pydantic import BaseModel


class PendingShipment(BaseModel):
    order_id: str


class DispatchedShipment(BaseModel):
    order_id: str
    tracking_number: str


Shipment = PendingShipment | DispatchedShipment
```

## Design Exception

When `None` is a real domain value rather than a hidden state:

```python
class Person(BaseModel):
    middle_name: str | None  # ddd-lint: ignore[DM001] - A person may have no middle name
```

Prefer the narrowest field-level exception. Use `DM002` instead when the team permits isolated nullable fields but wants to detect nullable-heavy models.
