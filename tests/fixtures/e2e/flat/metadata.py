from datetime import datetime

from pydantic import BaseModel
from third_party import DynamicModel


class Selection(BaseModel):
    selected_by: str | None  # ddd-lint: ignore[DM001] - covered by metadata pair policy
    selected_at: datetime | None  # ddd-lint: ignore[DM001] - covered by metadata pair policy


class SelectionMetadata(BaseModel):
    actor: str
    timestamp: datetime


class GroupedSelection(BaseModel):
    selection: SelectionMetadata | None  # ddd-lint: ignore[DM001] - complete selection may be absent


# ddd-lint: ignore[DM004] - System events may lack actor attribution
class SystemEvent(BaseModel):
    created_by: str | None  # ddd-lint: ignore[DM001] - actor may be unknown
    created_at: datetime | None  # ddd-lint: ignore[DM001] - event may be absent


class ExternalEvent(DynamicModel):
    created_by: str | None
    created_at: datetime | None
