from alpha.models import Document, Draft


def label(document: Document) -> str:
    match document:
        case Draft():
            return "draft"
