"""Tests for check_test_budgets.py: python3 -B -m unittest discover -s scripts"""

import contextlib
import io
import json
import pathlib
import sys
import tempfile
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import check_test_budgets as budgets  # noqa: E402


def vitest_report(dir, *tests):
    report = pathlib.Path(dir) / "vitest.json"
    results = [{"fullName": name, "status": "passed", "duration": wall, "meta": meta} for name, wall, meta in tests]
    report.write_text(json.dumps({"testResults": [{"name": "/ui/src/a.test.ts", "assertionResults": results}]}))
    return report


class UiSuite(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.addCleanup(self.dir.cleanup)
        self.addCleanup(setattr, budgets, "ROOT", budgets.ROOT)
        self.addCleanup(setattr, budgets, "BUDGETS", budgets.BUDGETS)
        budgets.ROOT = pathlib.Path(self.dir.name)
        budgets.BUDGETS = {"ui": {"report": "vitest.json", "total_ms": 100, "per_test_ms": 50}}

    def check(self):
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            ok = budgets.check("ui")
        return ok, out.getvalue(), err.getvalue()

    def test_counts_cpu_time_not_wall_time(self):
        vitest_report(self.dir.name, ("slow wall", 900, {"cpuMs": 40}), ("fast", 5, {"cpuMs": 30}))
        ok, out, err = self.check()
        self.assertTrue(ok, err)
        self.assertIn("2 tests, 70 ms CPU time of 100 ms budget", out)
        self.assertIn("slowest (CPU time)", out)

    def test_fails_a_test_over_its_cpu_budget(self):
        vitest_report(self.dir.name, ("busy", 10, {"cpuMs": 60}))
        ok, _, err = self.check()
        self.assertFalse(ok)
        self.assertIn("busy took 60 ms CPU time, per-test budget 50 ms", err)

    def test_stops_when_a_run_test_has_no_cpu_time(self):
        vitest_report(self.dir.name, ("unmeasured", 10, {}))
        with self.assertRaises(SystemExit) as stop:
            self.check()
        self.assertIn("unmeasured has no meta.cpuMs", str(stop.exception.code))


if __name__ == "__main__":
    unittest.main()
