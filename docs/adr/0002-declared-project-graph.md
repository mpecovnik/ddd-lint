# 0002: Resolve Declared Types Through One Project Graph

- Status: Accepted
- Date: 2026-07-27

## Context

Closed-type rules need more than a file-local AST. Real Python code imports union aliases, enum declarations, model bases, attributes, and annotated return types from other modules. A full Python type checker would provide broader inference but would dominate project scope and duplicate specialized tools.

uv workspaces contain multiple interconnected packages with one root and shared dependency environment. An isolated graph per package would fail to resolve normal workspace imports. Per-file or per-member configuration would also make one semantic graph subject to conflicting policies.

## Decision

Build one first-party project graph from declared syntax.

The graph resolves project imports, aliases, annotations, inheritance, known attributes, simple name aliases, and annotated function returns. It does not attempt general type inference. When required facts remain unresolved, rules skip the diagnostic.

Normal projects use `.` and `src` as default import roots. uv workspaces expand root `members` and `exclude` globs, add each member's `.` and `src` roots, and analyze all members in one graph. The workspace root owns the single `[tool.ddd-lint]` policy.

`.pyi` files contribute declarations but never receive DDD Lint rule or suppression diagnostics. Syntax errors still fail with a source error because the graph cannot index the declaration reliably.

## Consequences

- Cross-module and cross-workspace rules work without invoking Python, Pydantic, or uv.
- Findings depend on declared types and remain deterministic.
- Dynamic Python constructs and unresolved third-party declarations create false negatives rather than speculative findings.
- Workspace root configuration becomes a public policy boundary.
- Duplicate importable module names must fail clearly.
- Path-limited checks still index supporting workspace files.
- A future full type-checker integration can replace or enrich resolution behind the graph APIs without changing rule contracts.
