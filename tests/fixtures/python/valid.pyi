"""Exercise valid stub parsing and declaration indexing.

This fixture verifies that `.pyi` files preserve Literal annotations, overload
decorators, ellipsis bodies, unions, and nullable return types. Stubs contribute
declarations to the project graph but will not receive lint diagnostics.
"""

from typing import Literal, overload


class Draft:
    kind: Literal["draft"]


@overload
def load(identifier: int) -> Draft: ...


@overload
def load(identifier: str) -> Draft | None: ...
