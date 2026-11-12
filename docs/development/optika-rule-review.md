# Optika rule review

Checked `/Users/maticpecovnik/Work/optika/optika` with the local debug binary. Optika files were not edited. Results include its existing configuration exclusions and suppressions.

| Profile | Findings | Blocking warnings | Nonblocking reviews |
| --- | ---: | ---: | ---: |
| recommended | 8 | 6 | 2 |
| review | 228 | 6 | 222 |
| strict-construction, raw | 867 | 865 | 2 |

These are profile comparisons, not before/after measurements against an old binary.

## Counts by rule code

| Code | Finding | Recommended | Review | Strict construction (raw) |
| --- | --- | ---: | ---: | ---: |
| DM001 | Nullable field | 0 | 0 | 341 |
| DM002 | Nullable concentration | 0 | 31 | 0 |
| DM003 | Defaulted field | 0 | 0 | 518 |
| DM004 | Metadata pair | 4 | 4 | 4 |
| DM007 | Generated default | 0 | 30 | 0 |
| DM010 | State-dependent payload | 4 | 4 | 4 |
| DM011 | Implicit default choice | 0 | 159 | 0 |
| **Total** | | **8** | **228** | **867** |

Zero means no reported findings for that profile; the rule may be disabled. Other codes produced no findings.

Recommended findings:

- Four DM004 metadata pairs: NetworkStudySummaryResponse, JobOperationsOverviewResponse, UserResponse, and test FinalizationSelection.
- Two high-confidence DM010 validator-backed relationships: BindingOutcome and NovaOptions.
- Two low-confidence DM010 review suggestions: PreviewLayerSummary and _GroupKey.
- No DI001 findings. Frozen scalar LocalPort and empty frozenset defaults are excluded.

Review adds 159 implicit-default choices, 30 generation findings, and 31 nullable-concentration findings. Strict raw output contains 518 default-policy findings and 341 nullable-field findings.

Initial broad Literal/nullable coexistence detection also suggested architecture, CPU, severity, and unit fields. The final heuristic requires state/status naming when no validator evidence exists. Validator-backed findings do not depend on field naming.

No DM008/DM009 findings occurred in this checkout. Regression fixtures verify both positive detections and rejection of unrelated methods. Constructor combinations and consumer assertions remain outside this implementation's inference boundary.

## Enum-member default follow-up

After extending singleton-default matching to resolved enum members, the current checkout contains 49 direct matching enum-style Literal defaults and none remain flagged by DM003. Regression coverage includes imported/re-exported enum classes, type aliases, Field positional/keyword defaults, Annotated metadata, IntEnum and StrEnum, plus mismatched members and underlying scalar values.

The follow-up strict/raw run reports DM003: 461, DM001: 340, DM004: 4, DM010: 4. The checkout also changed between runs; these totals are not an isolated before/after measurement of the fix.
