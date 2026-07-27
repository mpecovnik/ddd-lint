# 0001: Build a Standalone CLI on the Ruff Parser

- Status: Accepted
- Date: 2026-07-27

## Context

DDD Lint needs modern Python syntax support, precise source ranges, fast parsing, and Ruff-like user experience. Ruff has no third-party rule plugin API. Adding these rules therefore requires a standalone tool, a maintained Ruff fork, or an upstream contribution whose acceptance and release policy would remain outside this project.

Parser candidates were `ruff_python_parser`, `rustpython-parser`, and a vendored parser. Ruff's parser provides the closest syntax and source-location behavior to the intended user experience. Its published crates are MIT licensed and current, but explicitly described as internal components, use `0.0.x` versions, and may change frequently.

## Decision

Build `ddd-lint` as a standalone Rust CLI and distribute it primarily through PyPI binary wheels.

Use exact-pinned `ruff_python_parser`, `ruff_python_ast`, and supporting Ruff source-range crate versions behind a project-owned syntax adapter. No rule or analyzer module may depend directly on Ruff types outside that adapter boundary.

Do not fork Ruff, vendor its parser, or design v1 around upstream acceptance.

## Consequences

- DDD Lint controls rule philosophy, release cadence, configuration, and suppressions.
- Users run DDD Lint alongside Ruff rather than loading it into Ruff.
- Parser behavior remains closely aligned with Ruff.
- Exact pins and adapter tests are required for every Ruff crate upgrade.
- Ruff parser API churn is contained but not eliminated.
- The selected crates set Rust 1.95 as the initial minimum toolchain.
- The project must implement its own CLI, discovery, diagnostics, configuration, and packaging.
