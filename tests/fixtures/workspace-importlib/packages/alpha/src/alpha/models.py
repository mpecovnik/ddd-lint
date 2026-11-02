from pydantic import BaseModel


class Model(BaseModel):
    pass


class Draft:
    pass


class Published:
    pass


Document = Draft | Published
