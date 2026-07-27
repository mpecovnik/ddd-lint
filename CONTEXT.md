# DDD Lint

DDD Lint identifies Python structures that often indicate weak domain modeling. Its rules are opinionated proxies: they expose design choices for review without claiming to prove whether a domain model is correct.

## Language

**Domain-model proxy**:
A syntactic condition correlated with a domain-modeling concern but insufficient to prove a design defect on its own.
_Avoid_: DDD law, correctness proof

**Closed type**:
A type whose possible alternatives can be completely enumerated. Class unions, literal types, and non-flag enums can be closed types.
_Avoid_: Progressive union

**Discriminated union**:
A union whose alternatives are distinguished by a stable discriminator. Progression, ordering, and forward-only transitions are not implied.
_Avoid_: Progressive union

**Nullable field**:
A model field whose declared type explicitly admits `None`. Omission and defaulting are separate concepts.
_Avoid_: Optional field

**Defaulted field**:
A model field that callers may omit because its declaration supplies a value or factory.
_Avoid_: Optional field

**Design exception**:
A consciously accepted rule violation documented with a specific rule code and reason.
_Avoid_: Exemption, blanket ignore

**Hidden constructor fallback**:
A constructor policy that replaces an omitted argument with a call-created collaborator instead of requiring composition to supply that collaborator explicitly.
_Avoid_: Convenient default
