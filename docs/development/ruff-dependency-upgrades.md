# Ruff Dependency Upgrades

DDD Lint uses Ruff component crates as parser implementation details. Their `0.0.x` APIs are unstable, so every Ruff component is pinned to one exact version in `Cargo.toml`.

Current pins:

```toml
ruff_python_ast = "=0.0.6"
ruff_python_parser = "=0.0.6"
ruff_text_size = "=0.0.6"
```

## Boundary

Only `src/python.rs` may import Ruff crates. The rest of DDD Lint consumes adapter-owned types such as `ParsedModule`, `SourceFile`, `ByteRange`, `SyntaxNode`, and `SyntaxKind`.

`tests/python_adapter.rs` enforces this boundary by scanning other Rust source files for Ruff imports.

## Upgrade Procedure

1. Read Ruff release changes and confirm the new crates support the project's Rust 1.95 minimum version.
2. Update all direct Ruff component pins together. Do not use compatible or wildcard version ranges.
3. Run `cargo update` and inspect `Cargo.lock` for unexpected dependency or license changes.
4. Adapt Ruff-specific code only inside `src/python.rs`.
5. Run `cargo fmt --check`.
6. Run `cargo clippy --all-targets -- -D warnings`.
7. Run `cargo test --all-targets`.
8. Confirm adapter tests still cover Python 3.10 through 3.15 syntax, `.pyi` parsing, comments, Unicode and multiline ranges, malformed sources, and unsupported-version errors.
9. Build a release wheel with `maturin build --release` and run the installed CLI smoke checks.
10. Record intentional adapter behavior changes in the pull request and update ADR 0001 if the boundary changes.

An upgrade is incomplete if a Ruff type appears outside `src/python.rs` or if an adapter test is removed without equivalent coverage.
