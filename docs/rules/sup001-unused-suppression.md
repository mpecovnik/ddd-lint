# SUP001: Unused Suppression

Rule name: `unused-suppression`

Origin: suppression hygiene.

Default: disabled.

## Summary

Report each rule code in a valid design exception that suppresses no enabled diagnostic in its scope.

## Why

Stale suppressions mislead readers about accepted design trade-offs. They can remain after code changes, rule configuration changes, or refactoring. Removing them keeps exception reasons aligned with current code.

`SUP001` evaluates only enabled rules. Running a deliberately narrow rule subset does not make unrelated design exceptions invalid unless `SUP001` is also selected under that same policy.

The rule checks source directives and structured `per-file-ignores` per listed code. A code is unused when it suppresses no enabled diagnostic in scope. Every code in a configuration entry is unused when its pattern matches no analyzed file. Source findings point to the directive; configuration findings point to the TOML entry.

## Example

Problematic when `DM001` is enabled because `name` is not nullable:

```python
class Customer(BaseModel):
    name: str  # ddd-lint: ignore[DM001] - Imported records may omit names
```

Prefer removing the stale directive:

```python
class Customer(BaseModel):
    name: str
```

Malformed directives, unknown rule codes, missing reasons, and invalid placement are always errors. They do not depend on `SUP001`.

## Design Exception

`SUP001` cannot appear in a source directive or `per-file-ignores`; either form is invalid because it would let a suppression prove its own use. Disable `SUP001` through project-level `ignore` when checking a policy profile that intentionally retains exceptions for other enabled-rule sets.
