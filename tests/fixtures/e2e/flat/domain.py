from enum import Enum
from typing import Literal


class Draft:
    kind: Literal["draft"]


class Published:
    kind: Literal["published"]


Document = Draft | Published


class Stage(Enum):
    DRAFT = "draft"
    PUBLISHED = "published"


def current_document() -> Document:
    return Draft()
