#!/usr/bin/env python3
"""Fail when a test suite, or a single test, is slower than its budget.

Usage: check_test_budgets.py SUITE [SUITE...]   (suites: rust, ui, e2e)
Budgets live in scripts/test-budgets.json. Always prints the slowest tests so
growth is visible before it becomes a failure.
"""

import json
import pathlib
import sys
import xml.etree.ElementTree as ET

ROOT = pathlib.Path(__file__).resolve().parent.parent
BUDGETS = json.loads((ROOT / "scripts" / "test-budgets.json").read_text())


def rust(report):
    tests = []
    for case in ET.parse(report).getroot().iter("testcase"):
        name = f"{case.get('classname')}::{case.get('name')}"
        tests.append((name, float(case.get("time", 0)) * 1000))
    return tests


def ui(report):
    data = json.loads(report.read_text())
    return [
        (f"{pathlib.Path(f['name']).name} > {t['fullName']}", t.get("duration") or 0)
        for f in data["testResults"]
        for t in f["assertionResults"]
    ]


def e2e(report):
    tests = []

    def walk(suite, prefix):
        title = f"{prefix} > {suite['title']}" if prefix else suite["title"]
        for spec in suite.get("specs", []):
            for test in spec["tests"]:
                ms = sum(r.get("duration", 0) for r in test["results"])
                tests.append((f"{title} > {spec['title']}", ms))
        for child in suite.get("suites", []):
            walk(child, title)

    for suite in json.loads(report.read_text())["suites"]:
        walk(suite, "")
    return tests


PARSERS = {"rust": rust, "ui": ui, "e2e": e2e}


def check(suite):
    budget = BUDGETS[suite]
    report = ROOT / budget["report"]
    if not report.exists():
        print(f"test-budget[{suite}]: report {budget['report']} missing", file=sys.stderr)
        return False
    tests = PARSERS[suite](report)
    total = sum(ms for _, ms in tests)
    ok = True
    print(f"test-budget[{suite}]: {len(tests)} tests, {total:.0f} ms of {budget['total_ms']} ms budget")
    for name, ms in sorted(tests, key=lambda t: -t[1])[:5]:
        print(f"  {ms:8.0f} ms  {name}")
    if total > budget["total_ms"]:
        print(f"error: {suite} suite took {total:.0f} ms, budget {budget['total_ms']} ms", file=sys.stderr)
        ok = False
    for name, ms in tests:
        if ms > budget["per_test_ms"]:
            print(f"error: {name} took {ms:.0f} ms, per-test budget {budget['per_test_ms']} ms", file=sys.stderr)
            ok = False
    return ok


if __name__ == "__main__":
    suites = sys.argv[1:] or list(PARSERS)
    results = [check(s) for s in suites]
    sys.exit(0 if all(results) else 1)
