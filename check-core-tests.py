#!/usr/bin/env python3
"""Run the core spec suite and enforce its committed assertion baseline."""

from __future__ import annotations

import argparse
import shutil
import subprocess
import sys
from collections import Counter
from pathlib import Path


ROOT = Path(__file__).resolve().parent
RESULTS_DIR = ROOT / "target" / "core-test-results"
BASELINE = ROOT / "tests" / "core-tests-baseline.txt"
SPEC_DIR = ROOT / "tests" / "spec"


def run_suite() -> None:
    shutil.rmtree(RESULTS_DIR, ignore_errors=True)
    result = subprocess.run(
        ["cargo", "test", "--features", "core-tests", "--test", "core_tests"],
        cwd=ROOT,
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )

    expected = {path.stem for path in SPEC_DIR.glob("*.wast")}
    produced = {path.stem for path in RESULTS_DIR.glob("*.tsv")}
    missing = sorted(expected - produced)
    if missing:
        print(result.stdout, file=sys.stderr)
        preview = ", ".join(missing[:10])
        suffix = f" and {len(missing) - 10} more" if len(missing) > 10 else ""
        raise SystemExit(f"core test runner did not produce results for {preview}{suffix}")


def read_results() -> tuple[dict[str, str], Counter[str], int]:
    assertions: dict[str, str] = {}
    skip_reasons: Counter[str] = Counter()
    runner_errors = 0

    for path in sorted(RESULTS_DIR.glob("*.tsv")):
        for line in path.read_text().splitlines():
            fields = line.split("\t")
            if fields[0] == "assertion":
                _, status, assertion_id, *_ = fields
                if assertion_id in assertions:
                    raise SystemExit(f"duplicate core assertion id: {assertion_id}")
                assertions[assertion_id] = status
                if status == "skipped":
                    skip_reasons[fields[3]] += 1
            elif fields[0] == "directive":
                skip_reasons[fields[3]] += 1
            elif fields[0] == "runner_error":
                runner_errors += 1
            else:
                raise SystemExit(f"unknown core result record in {path}: {line}")

    return assertions, skip_reasons, runner_errors


def read_baseline() -> tuple[int, set[str], bool]:
    if not BASELINE.is_file():
        raise SystemExit(
            "core test baseline is missing; run `python3 check-core-tests.py --update`"
        )

    passed = None
    passing_assertions = set()
    legacy = False
    for line in BASELINE.read_text().splitlines():
        fields = line.split("\t", 1)
        if fields[0] == "passed_assertions":
            passed = int(fields[1])
        elif fields[0] == "passing_assertion":
            passing_assertions.add(fields[1])
        elif fields[0] == "passing_case":
            legacy = True

    if passed is None:
        raise SystemExit("core test baseline is missing the passed assertion count")

    return passed, passing_assertions, legacy


def regressions(
    assertions: dict[str, str],
    baseline_passed: int,
    baseline_assertions: set[str],
) -> list[str]:
    passed = sum(status == "passed" for status in assertions.values())
    failures = []
    if passed < baseline_passed:
        failures.append(
            f"passed assertions regressed from {baseline_passed} to {passed}"
        )

    for assertion_id in sorted(baseline_assertions):
        status = assertions.get(assertion_id, "missing")
        if status != "passed":
            failures.append(
                f"previously passing assertion {assertion_id} is now {status}"
            )

    return failures


def write_baseline(assertions: dict[str, str]) -> None:
    passing = sorted(
        assertion_id
        for assertion_id, status in assertions.items()
        if status == "passed"
    )
    contents = [f"passed_assertions\t{len(passing)}"]
    contents.extend(f"passing_assertion\t{assertion_id}" for assertion_id in passing)
    BASELINE.write_text("\n".join(contents) + "\n")
    print(f"updated core test baseline: {len(passing)} passing assertions")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--update",
        action="store_true",
        help="record improvements in the committed baseline",
    )
    args = parser.parse_args()

    run_suite()
    assertions, skip_reasons, runner_errors = read_results()
    passed = sum(status == "passed" for status in assertions.values())
    failed = sum(status == "failed" for status in assertions.values())
    skipped = sum(status == "skipped" for status in assertions.values())
    print(
        f"core spec assertions: {passed} passed, {failed} failed, {skipped} skipped; "
        f"{runner_errors} runner errors"
    )
    for reason, count in sorted(skip_reasons.items()):
        print(f"  skipped {count}: {reason}")

    if BASELINE.is_file():
        baseline_passed, baseline_assertions, legacy = read_baseline()
        failures = [] if args.update and legacy else regressions(
            assertions, baseline_passed, baseline_assertions
        )
        if failures:
            print("core test baseline regressed:", file=sys.stderr)
            for failure in failures[:20]:
                print(f"  {failure}", file=sys.stderr)
            if len(failures) > 20:
                print(f"  ... and {len(failures) - 20} more", file=sys.stderr)
            return 1
    else:
        legacy = False

    if args.update:
        write_baseline(assertions)
        return 0

    baseline_passed, baseline_assertions, legacy = read_baseline()
    if legacy:
        print(
            "core test baseline still uses case ids; run "
            "`python3 check-core-tests.py --update`",
            file=sys.stderr,
        )
        return 1

    print(
        f"core test baseline preserved: {passed} passing assertions "
        f"(minimum {baseline_passed}); "
        f"{len(baseline_assertions)} passing assertions retained"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
