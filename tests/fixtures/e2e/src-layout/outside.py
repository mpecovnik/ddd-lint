from pydantic import BaseModel


class OutsideConfiguredRoots(BaseModel):
    missing: str | None
