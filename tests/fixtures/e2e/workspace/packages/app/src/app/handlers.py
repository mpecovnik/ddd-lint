from domain.events import AccountEvent, Opened


def render(event: AccountEvent) -> str:
    match event:
        case Opened():
            return "opened"
