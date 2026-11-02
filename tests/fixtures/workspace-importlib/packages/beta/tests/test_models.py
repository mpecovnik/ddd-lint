from alpha.models import Document, Draft

from .helpers import Helper


class Local(Helper):
    updated_by: str | None
    updated_at: str | None


def label(document: Document) -> str:
    match document:
        case Draft():
            return "draft"
