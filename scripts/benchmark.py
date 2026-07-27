#!/usr/bin/env python3
"""Generate deterministic repositories and measure cacheless CLI processes."""

from __future__ import annotations

import argparse
import json
import platform
import os
import statistics
import subprocess
import tempfile
import time
from pathlib import Path


SOURCE = '''from typing import Literal
from pydantic import BaseModel

class Draft:
    kind: Literal["draft"]

class Published:
    kind: Literal["published"]

Event = Draft | Published

class Payload(BaseModel):
    first: str | None
    second: str | None
    third: str | None
    retries: int = 3

def dispatch(event: Event) -> str:
    if isinstance(event, Draft):
        return "draft"
    match event:
        case Draft():
            return "draft"
        case _:
            return "other"

class Repository:
    pass

class Service:
    def __init__(self, repository=None):
        self.repository = repository or Repository()
'''


def parse_sizes(value: str) -> list[int]:
    sizes = [int(item) for item in value.split(",")]
    if not sizes or any(size < 1 for size in sizes):
        raise argparse.ArgumentTypeError("sizes must be positive comma-separated integers")
    return sizes


def create_repository(root: Path, size: int) -> int:
    (root / "pyproject.toml").write_text(
        '[tool.ddd-lint]\nselect = ["ALL"]\n', encoding="utf-8"
    )
    for index in range(size):
        (root / f"module_{index:06}.py").write_text(SOURCE, encoding="utf-8")
    return len(SOURCE.encode("utf-8")) * size


def measure(binary: Path, sizes: list[int], runs: int) -> dict[str, object]:
    results = []
    with tempfile.TemporaryDirectory(prefix="ddd-lint-benchmark-") as directory:
        benchmark_root = Path(directory)
        for size in sizes:
            repository = benchmark_root / f"files-{size}"
            repository.mkdir()
            source_bytes = create_repository(repository, size)
            samples = []
            for _ in range(runs):
                started = time.perf_counter()
                completed = subprocess.run(
                    [str(binary), "check", "--output-format", "json"],
                    cwd=repository,
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.PIPE,
                    check=False,
                )
                samples.append((time.perf_counter() - started) * 1000)
                if completed.returncode != 1:
                    raise RuntimeError(
                        f"benchmark exited {completed.returncode}: "
                        f"{completed.stderr.decode('utf-8', errors='replace')}"
                    )
            results.append(
                {
                    "files": size,
                    "source_bytes": source_bytes,
                    "runs": runs,
                    "median_ms": round(statistics.median(samples), 1),
                    "min_ms": round(min(samples), 1),
                    "max_ms": round(max(samples), 1),
                }
            )
    return {
        "binary": str(binary),
        "machine": platform.machine(),
        "platform": platform.platform(),
        "python": platform.python_version(),
        "results": results,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--binary",
        type=Path,
        default=Path("target/release") / ("ddd-lint.exe" if os.name == "nt" else "ddd-lint"),
        help="release binary to measure (default: target/release/ddd-lint[.exe])",
    )
    parser.add_argument(
        "--sizes",
        type=parse_sizes,
        default=parse_sizes("100,1000,5000"),
        help="comma-separated Python file counts",
    )
    parser.add_argument("--runs", type=int, default=5, help="processes per size")
    arguments = parser.parse_args()
    if arguments.runs < 1:
        parser.error("--runs must be positive")
    binary = arguments.binary.resolve()
    if not binary.is_file():
        parser.error(f"binary does not exist: {binary}; run cargo build --release")
    print(json.dumps(measure(binary, arguments.sizes, arguments.runs), indent=2))


if __name__ == "__main__":
    main()
