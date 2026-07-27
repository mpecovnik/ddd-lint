# Exercise stable syntax-error conversion for Python stub files.
# The unfinished base-class list and annotated line are intentionally invalid.
# Adapter tests verify that malformed stubs fail instead of entering the index.

class Broken(
    value: str
