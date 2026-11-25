#!/usr/bin/env python3
"""Fail when a test suite, or a single test, is slower than its budget.

Usage: check_test_budgets.py SUITE [SUITE...]   (suites: rust, ui, e2e)
Budgets live in scripts/test-budgets.json. Always prints the slowest tests so
growth is visible before it becomes a failure. The ui suite is measured in CPU
time per test (`meta.cpuMs`, written by ui/src/test-setup.ts), so load from
other work on the machine does not fail it; the others in wall time.
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
    tests = []
    for f in json.loads(report.read_text())["testResults"]:
        for t in f["assertionResults"]:
            name = f"{pathlib.Path(f['name']).name} > {t['fullName']}"
            cpu_ms = (t.get("meta") or {}).get("cpuMs")
            if cpu_ms is None and t["status"] in ("passed", "failed"):
                sys.exit(f"test-budget[ui]: {name} has no meta.cpuMs; is ui/src/test-setup.ts loaded?")
            tests.append((name, cpu_ms or 0))
    return tests


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
CLOCKS = {"ui": "CPU time"}


def check(suite):
    budget = BUDGETS[suite]
    report = ROOT / budget["report"]
    if not report.exists():
        print(f"test-budget[{suite}]: report {budget['report']} missing", file=sys.stderr)
        return False
    tests = PARSERS[suite](report)
    total = sum(ms for _, ms in tests)
    clock = CLOCKS.get(suite, "wall time")
    ok = True
    print(f"test-budget[{suite}]: {len(tests)} tests, {total:.0f} ms {clock} of {budget['total_ms']} ms budget")
    print(f"  slowest ({clock}):")
    for name, ms in sorted(tests, key=lambda t: -t[1])[:5]:
        print(f"  {ms:8.0f} ms  {name}")
    if total > budget["total_ms"]:
        print(f"error: {suite} suite took {total:.0f} ms {clock}, budget {budget['total_ms']} ms", file=sys.stderr)
        ok = False
    for name, ms in tests:
        if ms > budget["per_test_ms"]:
            print(f"error: {name} took {ms:.0f} ms {clock}, per-test budget {budget['per_test_ms']} ms", file=sys.stderr)
            ok = False
    return ok


if __name__ == "__main__":
    suites = sys.argv[1:] or list(PARSERS)
    results = [check(s) for s in suites]
    sys.exit(0 if all(results) else 1)
