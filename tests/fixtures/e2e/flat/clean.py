from pydantic import BaseModel

from domain import Document, Draft, Published


class RequiredOrder(BaseModel):
    identifier: str


class SmallRecord(BaseModel):
    note: str | None  # ddd-lint: ignore[DM001] - single nullable value is intentional
    identifier: str


class InjectedService:
    def __init__(self, repository):
        self.repository = repository


def exhaustive(document: Document) -> str:
    match document:
        case Draft():
            return "draft"
        case Published():
            return "published"
