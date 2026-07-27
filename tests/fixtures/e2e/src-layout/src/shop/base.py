from pydantic import BaseModel


class DomainModel(BaseModel):
    identifier: str
