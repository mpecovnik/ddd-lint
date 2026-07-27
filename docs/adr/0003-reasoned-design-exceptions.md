# 0003: Require Reasons for Scoped Design Exceptions

- Status: Accepted
- Date: 2026-07-27

## Context

Many DDD Lint rules are intentionally opinionated proxies. Legitimate nullable fields, defaults, protocol adapters, and composition choices exist. Suppression is therefore a required part of correct use, not an exceptional implementation detail.

Reusing `# noqa` would conflict with Ruff's own code validation and unused-suppression checks. Blanket ignores and unreasoned path exclusions make policy erosion hard to review and allow newly added rules to disappear silently.

## Decision

Use project-owned `# ddd-lint:` directives.

Every source directive must list explicit rule codes and include a non-empty reason after the mandatory ` - ` delimiter. Directives may cover a physical line, logical statement, function/class subtree, or file according to the lexical attachment grammar in the v1 design. Configuration-level per-file ignores use structured entries containing `pattern`, `rules`, and `reason`.

Blanket suppressions are unsupported. Malformed directives are always source errors. Unused valid source or configuration suppressions are reported only when `SUP001` is selected and only relative to enabled diagnostics. `SUP001` itself cannot be locally suppressed; only project-level selection can disable it.

## Consequences

- Every accepted violation records local design intent.
- Future rules cannot be hidden by old blanket directives.
- DDD Lint avoids collisions with Ruff `noqa` handling.
- Suppression syntax becomes a stable public contract.
- Source and configuration migrations require more effort than unreasoned ignore lists.
- Generated or external-schema files should use reasoned per-file configuration or file directives rather than blanket exclusion when their declarations are still needed for indexing.
