# 0002: Resolve Declared Types Through One Project Graph

- Status: Accepted
- Date: 2026-07-27

## Context

Closed-type rules need more than a file-local AST. Real Python code imports union aliases, enum declarations, model bases, attributes, and annotated return types from other modules. A full Python type checker would provide broader inference but would dominate project scope and duplicate specialized tools.

uv workspaces contain multiple interconnected packages with one root and shared dependency environment. An isolated graph per package would fail to resolve normal workspace imports. Per-file or per-member configuration would also make one semantic graph subject to conflicting policies.

## Decision

Build one first-party project graph from declared syntax.

The graph resolves project imports, aliases, annotations, inheritance, known attributes, simple name aliases, and annotated function returns. It does not attempt general type inference. When required facts remain unresolved, rules skip the diagnostic.

Normal projects use `.` and `src` as default import roots. uv workspaces expand root `members` and `exclude` globs and analyze all members in one graph. The workspace root retains `.` and `src` roots and owns the single `[tool.ddd-lint]` policy.

As clarified for [issue #2](https://github.com/mpecovnik/ddd-lint/issues/2), each member contributes only its `src/` directory when present, otherwise its member directory. A file's deepest enclosing member owns its layout; ancestor member roots do not apply. Files outside that member's application root use workspace-relative identities. This keeps sibling test trees distinct without excluding them, including repeated `tests/conftest.py` and test helper names in src-layout workspaces. Relative test imports remain local, while application imports share the project graph. Pytest configuration is not required to select this policy.

`.pyi` files contribute declarations but never receive DDD Lint rule or suppression diagnostics. Syntax errors still fail with a source error because the graph cannot index the declaration reliably.

## Consequences

- Cross-module and cross-workspace rules work without invoking Python, Pydantic, or uv.
- Findings depend on declared types and remain deterministic.
- Dynamic Python constructs and unresolved third-party declarations create false negatives rather than speculative findings.
- Workspace root configuration becomes a public policy boundary.
- Duplicate importable module names must fail clearly.
- Files beside a member's `src/` no longer become bare imports through an automatically added member-directory root.
- Path-limited checks still index supporting workspace files.
- A future full type-checker integration can replace or enrich resolution behind the graph APIs without changing rule contracts.
