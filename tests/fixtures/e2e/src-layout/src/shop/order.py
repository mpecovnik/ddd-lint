from shop.base import DomainModel


class Order(DomainModel):
    cancellation_reason: str | None
