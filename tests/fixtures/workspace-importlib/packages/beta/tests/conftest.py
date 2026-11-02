from alpha.models import Model


class Record(Model):
    archived_at: str | None
    archived_by: str | None
