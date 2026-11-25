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


def nextest_report(dir, cpu, *cases):
    """cases: (binary, test, wall_s, child_xml); cpu: lines of the wrapper's file."""
    report = pathlib.Path(dir) / "junit.xml"
    xml = "".join(f'<testcase classname="{b}" name="{t}" time="{w}">{c}</testcase>' for b, t, w, c in cases)
    report.write_text(f'<testsuites uuid="run1"><testsuite name="x">{xml}</testsuite></testsuites>')
    (pathlib.Path(dir) / "cpu").mkdir()
    (pathlib.Path(dir) / "cpu" / "run1.tsv").write_text("".join(f"{b}\t{t}\t{ms}\n" for b, t, ms in cpu))
    return report


class RustSuite(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.addCleanup(self.dir.cleanup)
        self.addCleanup(setattr, budgets, "ROOT", budgets.ROOT)
        self.addCleanup(setattr, budgets, "BUDGETS", budgets.BUDGETS)
        budgets.ROOT = pathlib.Path(self.dir.name)
        budgets.BUDGETS = {"rust": {"report": "junit.xml", "total_ms": 100, "per_test_ms": 50}}

    def check(self):
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            ok = budgets.check("rust")
        return ok, out.getvalue(), err.getvalue()

    def test_counts_cpu_time_not_wall_time(self):
        nextest_report(
            self.dir.name,
            [("core", "slow_wall", 40), ("core", "fast", 30)],
            ("core", "slow_wall", 0.9, ""),
            ("core", "fast", 0.005, ""),
        )
        ok, out, err = self.check()
        self.assertTrue(ok, err)
        self.assertIn("2 tests, 70 ms CPU time of 100 ms budget", out)

    def test_fails_a_test_over_its_cpu_budget(self):
        nextest_report(self.dir.name, [("core", "busy", 60)], ("core", "busy", 0.01, ""))
        ok, _, err = self.check()
        self.assertFalse(ok)
        self.assertIn("core::busy took 60 ms CPU time, per-test budget 50 ms", err)

    def test_a_killed_test_counts_its_wall_time(self):
        nextest_report(self.dir.name, [], ("core", "hangs", 3.0, "<failure/>"))
        ok, _, err = self.check()
        self.assertFalse(ok)
        self.assertIn("core::hangs took 3000 ms", err)

    def test_stops_when_a_passed_test_has_no_cpu_time(self):
        nextest_report(self.dir.name, [], ("core", "unmeasured", 0.01, ""))
        with self.assertRaises(SystemExit) as stop:
            self.check()
        self.assertIn("core::unmeasured has no CPU time", str(stop.exception.code))

    def test_drops_the_files_of_earlier_runs(self):
        nextest_report(self.dir.name, [("core", "t", 1)], ("core", "t", 0.01, ""))
        old = pathlib.Path(self.dir.name) / "cpu" / "run0.tsv"
        old.write_text("")
        self.check()
        self.assertFalse(old.exists())
        self.assertTrue((pathlib.Path(self.dir.name) / "cpu" / "run1.tsv").exists())


if __name__ == "__main__":
    unittest.main()
