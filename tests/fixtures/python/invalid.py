# Exercise stable syntax-error conversion for ordinary Python files.
# The missing parameter before the colon is intentional. Adapter tests assert
# normalized messages, Ruff details, byte ranges, and one-indexed locations.

def broken(:
    pass
