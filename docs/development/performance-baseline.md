# Performance Baseline

## Method

Run from repository root:

```shell
cargo build --release
python3 scripts/benchmark.py
```

The script deterministically generates repositories containing 100, 1,000, and 5,000 Python files. Every file contains a Pydantic model, a class union, match and conditional dispatch, and a constructor fallback. All rules are enabled. Each sample starts a new release-binary process, performs complete discovery, parsing, indexing, analysis, and JSON serialization, then discards output. DDD Lint has no persistent cache, so each sample is a cold application run. Filesystem cache state is uncontrolled; results must not be interpreted as cold-storage I/O measurements.

Five samples are collected per size. Median, minimum, and maximum wall-clock times form the regression baseline. Run on an otherwise idle machine and compare on equivalent hardware; virtualized CI timing is useful for trends, not direct comparison with this baseline.

## Measured Baseline

Measured 2026-07-27 from the MAT-14 working tree based on commit `24bb473cb656239befbefa7013f309b4eebb531c`. This pre-commit measurement is provisional: rerun it after the MAT-14 tree is committed and replace this provenance before using the values as a release regression gate.

- Apple M4 Pro, 24 GiB RAM
- macOS 26.5.2 arm64
- `rustc 1.95.0 (59807616e 2026-04-14)`
- Python 3.13.7 benchmark driver
- Release profile with thin LTO and stripped binary

| Python files | Source bytes | Runs | Median | Minimum | Maximum |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 100 | 62,900 | 5 | 25.8 ms | 25.4 ms | 26.9 ms |
| 1,000 | 629,000 | 5 | 1,036.4 ms | 1,002.8 ms | 1,040.0 ms |
| 5,000 | 3,145,000 | 5 | 23,845.4 ms | 23,141.9 ms | 23,989.0 ms |

Raw script JSON, with the local repository prefix normalized:

```json
{
  "binary": "<repository>/target/release/ddd-lint",
  "machine": "arm64",
  "platform": "macOS-26.5.2-arm64-arm-64bit-Mach-O",
  "python": "3.13.7",
  "results": [
    {
      "files": 100,
      "source_bytes": 62900,
      "runs": 5,
      "median_ms": 25.8,
      "min_ms": 25.4,
      "max_ms": 26.9
    },
    {
      "files": 1000,
      "source_bytes": 629000,
      "runs": 5,
      "median_ms": 1036.4,
      "min_ms": 1002.8,
      "max_ms": 1040.0
    },
    {
      "files": 5000,
      "source_bytes": 3145000,
      "runs": 5,
      "median_ms": 23845.4,
      "min_ms": 23141.9,
      "max_ms": 23989.0
    }
  ]
}
```

This baseline is descriptive, not a release threshold or user-facing speed claim.

## Known Dynamic Gaps

The analyzer skips facts it cannot prove through wildcard imports, dynamic class creation, monkey-patching, unresolved third-party declarations, or general data flow. Helper predicates and membership tests are outside `CT003`. Ambiguous or overlapping class unions and discriminator fields without distinct singleton literals are not treated as closed discriminated unions. Benchmark source uses only statically resolvable forms and therefore does not measure these skipped paths.
