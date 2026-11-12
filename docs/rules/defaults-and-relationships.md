# Defaults and relationships

These checks identify review opportunities; they do not prove a domain model is wrong. No model is exempted because its name ends in Response, DTO, or Settings.

## Defaults

Matching single-value Literal defaults are allowed, including resolved aliases, Field(default=...), and Annotated Field metadata. Mismatched defaults remain findings. Numeric equality preserves literal types: False is not exempted as Literal[0].

This includes singleton enum-member tags such as `Literal[State.READY] = State.READY`. Enum class/member identity is resolved through imports and re-exports; an equal underlying string or integer is not treated as the same enum member.

DM003 remains the opt-in strict construction policy. When enabled, it takes precedence over narrower default rules:

| Rule | Observation |
| --- | --- |
| DM005 | Explicit None default; usually legitimate absence |
| DM006 | Zero, False, empty list/tuple/dictionary literal; review initial state versus policy |
| DM007 | Call expression or Field default_factory; implicit generation |
| DM011 | Other defaults; review implicit choice |

No automatic exemption distinguishes retry_count=0 from failed_count=0. These rules classify syntax, not business meaning. Factories remain generation findings even when they return empty collections. Narrow default findings are nonblocking reviews; DM003 blocks.

## Correlated fields

- DM004 retains its narrow actor/time naming heuristic.
- DM008 recognizes all-or-none nullable fields enforced by an after model validator.
- DM009 recognizes exactly-one nullable fields enforced by an after model validator.
- DM010 recognizes multi-value Literal state fields with nullable payloads. State/status naming alone produces low-confidence, nonblocking review. Recognized after-validator guards tying literal state comparisons to None checks produce high-confidence findings.

Presence analysis evaluates explicit `is None` / `is not None` comparisons, boolean equality/inequality, and conjunction/disjunction over two to eight fields. It recognizes direct raising if statements in after validators. State analysis also recognizes nested positive if guards. Validators are resolved through imports; unrelated methods and decorators are not treated as Pydantic validators.

Messages say “invariant enforced at runtime; consider structural model,” not that invalid states are currently accepted. Unsupported predicates, before/wrap validators, collection-based checks, arbitrary control flow, constructor-use combinations, and consumer assertions are not inferred in this version. Missing evidence does not establish independence.

## Constructor defaults

DI001 excludes resolved scalar builtin constructors, empty frozenset(), and directly declared frozen scalar RootModel wrappers. RootModel recognition accepts frozen=True class keywords or ConfigDict(frozen=True), scalar root overrides, no extra fields, no multiple inheritance, and no methods. Mutable roots and arbitrary model constructors remain candidates. Unknown or shadowed constructors are not granted value exemptions.

DI002 is a separate opt-in Python correctness check for list(), dict(), and set() defaults in constructor signatures. These builtin collection calls are excluded from DI001, including per-call fallback allocations. DI002 currently does not inspect literal mutable defaults or non-constructor function signatures.

## Output and suppression

Suppression and stale-suppression analysis run before diagnostic deduplication. DM001 observations on correlated fields and DM002 concentration observations attach to a specific relationship finding. CT001 attaches to CT002 on the same match. Unrelated nullable fields remain visible. `--raw` returns independent findings.

JSON confidence describes available evidence, not certainty of a modeling defect. `severity: review` does not affect exit status. Existing warning diagnostics and source errors still produce exit code 1.
