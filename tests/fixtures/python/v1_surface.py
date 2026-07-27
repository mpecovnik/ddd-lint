"""Exercise every Python syntax family required by the v1 lint rules.

This fixture combines Pydantic models, nullable/defaulted fields, class unions,
enum values, conditional dispatch, exhaustive match patterns, annotations,
decorators, and hidden constructor fallbacks. Parser tests use it to verify that
the adapter emits all syntax kinds needed by the DM, CT, and DI rule families.
"""

from enum import Enum
from typing import Annotated, Literal

from pydantic import BaseModel, Field


class Draft(BaseModel):
    kind: Literal["draft"]
    title: str
    note: str | None = None  # intentional nullable field


class Published(BaseModel):
    kind: Literal["published"]
    title: str
    url: Annotated[str, Field(min_length=1)]


Document = Draft | Published


class Status(Enum):
    OPEN = "open"
    CLOSED = "closed"


def dispatch(document: Document, repository: Repository | None = None) -> str:
    if isinstance(document, Draft):
        return document.title

    match document:
        case Draft() as draft if draft.note:
            return draft.note
        case Draft() | Published():
            return document.title


def inspect_shape(value: object) -> None:
    match value:
        case None:
            pass
        case Status.OPEN:
            pass
        case [first, *rest]:
            pass
        case {"kind": kind, **remaining}:
            pass


@service
class Service:
    def __init__(self, repository: Repository | None = None):
        self.repository = repository or NoopRepository()
