from domain import Document, Draft, Published, Stage, current_document


def incomplete(document: Document) -> str:
    match document:
        case Draft():
            return "draft"


def catch_all(document: Document) -> str:
    match document:
        case Draft():
            return "draft"
        case Published():
            return "published"
        case _:
            return "unknown"


def conditional(document: Document) -> str:
    if isinstance(document, Draft):
        return "draft"
    return "published"


def by_discriminator(document: Document) -> str:
    match document.kind:
        case "draft":
            return "draft"
        case "published":
            return "published"


def from_return() -> str:
    document = current_document()
    match document:
        case Draft():
            return "draft"


def enum_stage(stage: Stage) -> str:
    match stage:
        case Stage.DRAFT:
            return "draft"
