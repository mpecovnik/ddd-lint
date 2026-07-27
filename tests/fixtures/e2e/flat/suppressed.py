from domain import Document, Draft, Published


# ddd-lint: ignore[CT001] - caller excludes published documents
def suppressed_incomplete(document: Document) -> str:
    match document:
        case Draft():
            return "draft"


def suppressed_catch_all(document: Document) -> str:
    match document:
        case Draft():
            return "draft"
        case Published():
            return "published"
        case _:  # ddd-lint: ignore[CT002] - adapter quarantines unknown runtime values
            return "unknown"


# ddd-lint: ignore[CT003] - external callback protocol requires branching
def suppressed_conditional(document: Document) -> str:
    if isinstance(document, Draft):
        return "draft"
    return "published"


# ddd-lint: ignore[CT004] - serialized protocol dispatches on its stable tag
def suppressed_discriminator(document: Document) -> str:
    match document.kind:
        case "draft":
            return "draft"
        case "published":
            return "published"


value = 1  # ddd-lint: ignore[DM003] - stale fixture for SUP001
