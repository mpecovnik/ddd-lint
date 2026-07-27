# CT004: Discriminator Match

Rule name: `discriminator-match`

Origin: discriminated-union modeling rule; DDD-adjacent.

Default: disabled.

## Summary

When a class union has a recognized discriminator, match the union object by member class instead of matching its discriminator value.

## Why

Matching the discriminator repeats representation details at behavior sites. Class patterns dispatch on modeled alternatives directly and preserve member-level exhaustiveness when tags or member internals change.

DDD Lint recognizes a structural discriminator when every class-union member declares the same field with a distinct singleton `Literal` value.

## Example

Problematic:

```python
from typing import Literal
from pydantic import BaseModel


class Draft(BaseModel):
    kind: Literal["draft"]
    title: str


class Published(BaseModel):
    kind: Literal["published"]
    title: str
    url: str


Document = Draft | Published


def location(document: Document) -> str:
    match document.kind:
        case "draft":
            return "local"
        case "published":
            return document.url
```

Prefer member class patterns:

```python
def location(document: Document) -> str:
    match document:
        case Draft():
            return "local"
        case Published(url=url):
            return url
```

`CT004` is separate from literal exhaustiveness. An exhaustive tag-value match still triggers because the policy concerns dispatch representation, not missing tags.

## Design Exception

Wire-format adapters may intentionally operate on discriminator values:

```python
# ddd-lint: ignore[CT004] - Serializer protocol is defined in terms of wire tags
match document.kind:
    case "draft":
        encode_draft(document)
    case "published":
        encode_published(document)
```
