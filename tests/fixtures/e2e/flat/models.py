from pydantic import BaseModel


class NullableOrder(BaseModel):
    note: str | None


class SparseOrder(BaseModel):
    first: str | None  # ddd-lint: ignore[DM001] - covered by aggregate-level policy
    second: str | None  # ddd-lint: ignore[DM001] - covered by aggregate-level policy
    third: str | None  # ddd-lint: ignore[DM001] - covered by aggregate-level policy
    required: str


class DefaultedOrder(BaseModel):
    retries: int = 3


# ddd-lint: ignore[DM002] - imported wire record remains intentionally sparse
class SuppressedOrder(BaseModel):
    optional_note: str | None  # ddd-lint: ignore[DM001] - absence is domain-significant
    optional_code: str | None  # ddd-lint: ignore[DM001] - absence is domain-significant
    optional_owner: str | None  # ddd-lint: ignore[DM001] - absence is domain-significant
    source: str = "partner"  # ddd-lint: ignore[DM003] - protocol supplies this constant
