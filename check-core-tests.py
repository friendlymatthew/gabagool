#!/usr/bin/env python3
"""Run the core spec suite and enforce its committed passing baseline."""

from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parent
RESULTS = ROOT / "target" / "core-test-results.tsv"
BASELINE = ROOT / "tests" / "core-tests-baseline.txt"


def run_suite() -> None:
    RESULTS.unlink(missing_ok=True)
    subprocess.run(
        ["cargo", "test", "--features", "core-tests", "--test", "core_tests"],
        cwd=ROOT,
        check=False,
    )

    if not RESULTS.is_file():
        raise SystemExit("core test runner did not produce target/core-test-results.tsv")


def read_results() -> tuple[int, dict[str, str]]:
    passed = None
    cases: dict[str, str] = {}

    for line in RESULTS.read_text().splitlines():
        fields = line.split("\t")
        if fields[0] == "passed_assertions":
            passed = int(fields[1])
        elif fields[0] == "case":
            _, status, name = fields
            if name in cases:
                raise SystemExit(f"duplicate core test case in results: {name}")
            cases[name] = status

    if passed is None:
        raise SystemExit("core test results are missing the passed assertion count")

    return passed, cases


def read_baseline() -> tuple[int, set[str]]:
    if not BASELINE.is_file():
        raise SystemExit(
            "core test baseline is missing; run `python3 check-core-tests.py --update`"
        )

    passed = None
    passing_cases = set()
    for line in BASELINE.read_text().splitlines():
        fields = line.split("\t", 1)
        if fields[0] == "passed_assertions":
            passed = int(fields[1])
        elif fields[0] == "passing_case":
            passing_cases.add(fields[1])

    if passed is None:
        raise SystemExit("core test baseline is missing the passed assertion count")

    return passed, passing_cases


def regressions(
    passed: int,
    cases: dict[str, str],
    baseline_passed: int,
    baseline_cases: set[str],
) -> list[str]:
    failures = []
    if passed < baseline_passed:
        failures.append(
            f"passed assertions regressed from {baseline_passed} to {passed}"
        )

    for name in sorted(baseline_cases):
        status = cases.get(name, "missing")
        if status != "ok":
            failures.append(f"previously passing case {name} is now {status}")

    return failures


def write_baseline(passed: int, cases: dict[str, str]) -> None:
    passing_cases = sorted(name for name, status in cases.items() if status == "ok")
    contents = [f"passed_assertions\t{passed}"]
    contents.extend(f"passing_case\t{name}" for name in passing_cases)
    BASELINE.write_text("\n".join(contents) + "\n")
    print(
        f"updated core test baseline: {passed} passed assertions and "
        f"{len(passing_cases)} passing cases"
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--update",
        action="store_true",
        help="record improvements in the committed baseline",
    )
    args = parser.parse_args()

    run_suite()
    passed, cases = read_results()

    if BASELINE.is_file():
        baseline_passed, baseline_cases = read_baseline()
        failures = regressions(passed, cases, baseline_passed, baseline_cases)
        if failures:
            print("core test baseline regressed:", file=sys.stderr)
            for failure in failures[:20]:
                print(f"  {failure}", file=sys.stderr)
            if len(failures) > 20:
                print(f"  ... and {len(failures) - 20} more", file=sys.stderr)
            return 1

    if args.update:
        write_baseline(passed, cases)
        return 0

    baseline_passed, baseline_cases = read_baseline()
    print(
        f"core test baseline preserved: {passed} passed assertions "
        f"(minimum {baseline_passed}); {len(baseline_cases)} passing cases retained"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
