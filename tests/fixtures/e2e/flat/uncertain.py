from third_party import DynamicModel


class DynamicRecord(DynamicModel):
    note: str | None
    retries: int = 3


class BaseEvent:
    pass


class ChildEvent(BaseEvent):
    kind: str


OverlappingEvent = BaseEvent | ChildEvent


def overlap(event: OverlappingEvent) -> str:
    match event:
        case _:
            return "event"


def helper_dispatch(event: OverlappingEvent) -> str:
    if is_child(event):
        return "child"
    return "base"


class Alpha:
    kind: str


class Beta:
    kind: str


AmbiguousEvent = Alpha | Beta


def ambiguous_discriminator(event: AmbiguousEvent) -> str:
    match event.kind:
        case _:
            return "event"


DEFAULT_REPOSITORY = object()


class DynamicService:
    def __init__(self, repository=DEFAULT_REPOSITORY):
        self.repository = repository


dynamic_value = 1  # ddd-lint: ignore[CT003] - uncertainty makes this intentionally stale
