class Opened:
    pass


class Closed:
    pass


AccountEvent = Opened | Closed


def domain_only(event: AccountEvent) -> str:
    match event:
        case Opened():
            return "opened"
