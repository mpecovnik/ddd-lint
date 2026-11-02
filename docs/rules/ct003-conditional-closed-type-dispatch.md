# CT003: Conditional Closed-Type Dispatch

Rule name: `conditional-closed-type-dispatch`

Origin: closed-type modeling rule; DDD-adjacent.

Default: disabled. When selected, `mode = "dispatch"`.

## Summary

Prefer exhaustive pattern matching for conditional sequences that select multiple distinct members of the same closed-type value. Single predicates and type guards are allowed in the default `dispatch` mode.

## Why

Conditional dispatch makes omitted alternatives easy to overlook. For example, checking two states and interpreting everything else as a third state silently assigns that interpretation to future states. Pattern matching gives the linter one structured dispatch site whose coverage can be checked against the closed type.

The default policy favors high-value findings with a low likelihood of false positives over completeness. Testing one member can be an intentional predicate, authorization whitelist, or guarded access. It does not by itself establish variant-by-variant dispatch.

`CT003` identifies conditional dispatch to rewrite; `CT001` checks explicit member coverage once it becomes a `match`; `CT002` rejects ordinary catch-all branches in that match. Enable those rules together for exhaustive dispatch enforcement.

## Dispatch Mode

Report one diagnostic at the first condition of either:

- An `if`/`elif` ladder with at least two member-selecting branches. An `else` is not required.
- A consecutive sequence of at least two standalone `if` statements in the same statement body, each without `elif`/`else` and each ending directly in `return` or `raise`. These are early-exit dispatch even though they do not use `elif`.

Every condition in the candidate sequence must be a recognized positive test on the same subject, and at least two distinct resolved members must be selected. Repeating one member does not qualify. Supported tests are:

- `isinstance(value, Member)` with one member class;
- `type(value) is Member` or `type(value) == Member`;
- enum and literal equality or identity comparisons;
- recognized discriminator-field equality or identity comparisons.

Reversed operands are supported. The subject must be the same name or name-rooted attribute chain, such as `document` or `request.document`; parentheses do not matter. Discriminator tests identify the underlying union value. Required declarations must resolve to a closed type, and builtin checks must be unshadowed.

The analysis deliberately skips:

- single member predicates, even with `else` or an early return;
- ternaries, boolean combinations, negated tests, inequality, chained comparisons, grouped membership tests, and helper predicates;
- independent non-terminating `if` statements;
- candidate sequences mixing subjects or unresolved tests;
- repeated calls or subscripts used as subjects;
- early-exit sequences interrupted by another statement or requiring nested control-flow reasoning to establish termination.

Assertions, loop conditions, comprehensions, and match guards are not dispatch sites. Conditional statements inside nested bodies are analyzed separately. This is structural recognition, not arbitrary boolean or control-flow analysis: skipped code is not certified exhaustive or safe. In particular, a single-check implicit fallback can still hide future members.

## Example: Class Union

Problematic:

```python
def label(document: Document) -> str:
    if isinstance(document, Draft):
        return "draft"
    elif isinstance(document, Published):
        return "published"
    return "archived"
```

Prefer:

```python
def label(document: Document) -> str:
    match document:
        case Draft():
            return "draft"
        case Published():
            return "published"
        case Archived():
            return "archived"
```

The equivalent early-return spelling also reports:

```python
if isinstance(document, Draft):
    return "draft"
if isinstance(document, Published):
    return "published"
return "archived"
```

## Example: Enum Dispatch

Problematic:

```python
if state == State.NOT_EVALUATED:
    return pending()
if state == State.FAILED:
    return failure()
return not_requested()
```

Prefer:

```python
match state:
    case State.NOT_EVALUATED:
        return pending()
    case State.FAILED:
        return failure()
    case State.NOT_REQUESTED:
        return not_requested()
```

## Allowed Predicates

These do not report in `dispatch` mode:

```python
# An authorization whitelist intentionally excludes future roles.
if user.is_platform_admin or (
    user.organization_id == organization_id and user.role == Role.ADMIN
):
    return user

# A type guard permits access to the readable variant's attributes.
if not isinstance(geometry, UnreadableGeometry) and not geometry.shape.intersects(extent.shape):
    reject(geometry)

# An intentional grouping, not a separate branch for each environment.
retention_days = 30 if environment == Environment.PRODUCTION else 7
```

Independent actions also remain allowed:

```python
if isinstance(document, Draft):
    record_draft_metric()
if isinstance(document, Published):
    record_published_metric()
```

## Strict Mode and Migration

The previous policy remains available explicitly:

```toml
[tool.ddd-lint]
select = ["CT"]

[tool.ddd-lint.rules.CT003]
mode = "strict"
```

`strict` reports every recognized direct member test in `if`, `elif`, and ternaries, including single early-return checks, tests inside boolean combinations, negation, and inequality. Assertions, loop conditions, comprehensions, and match guards remain outside that policy. It does not require a multi-branch sequence or a stable-reference subject.

`dispatch` is the new default for exact `CT003`, family `CT`, and `ALL` selectors. Recommended onboarding uses that default. Projects requiring the old strict policy must set `mode = "strict"`; enabling more rule codes alone does not select strict mode. Existing suppressions for single predicates may become unused under `SUP001` and should be removed or retained with strict mode as appropriate.

## Design Exception

Adapter code may need to mirror a third-party conditional protocol:

```python
# ddd-lint: ignore[CT003] - Vendor callback contract requires this fast-path branch
if event.kind == "heartbeat":
    acknowledge(event)
elif event.kind == "message":
    receive(event)
```

In dispatch mode, place the exception on or immediately above the first `if` to cover the sequence's single finding. Function-level and per-file exceptions also apply.
