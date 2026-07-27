from contracts import Delivered, Queued, Shipment, load_shipment


def direct(shipment: Shipment) -> str:
    match shipment:
        case Queued():
            return "queued"


def returned() -> str:
    shipment = load_shipment()
    match shipment:
        case Delivered():
            return "delivered"
