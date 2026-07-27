class MemoryRepository:
    pass


class Service:
    def __init__(self, repository=None):
        self.repository = repository or MemoryRepository()


class PreviewService:
    def __init__(self, repository=None):
        self.repository = repository or MemoryRepository()  # ddd-lint: ignore[DI001] - preview owns isolated storage
