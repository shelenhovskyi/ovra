import { describe, expect, it } from "vitest";
import { canRunAgain, checkText } from "./describe";
import { closerIntro, closerRows } from "./look-closer";

describe("look closer", () => {
  it("names what changed and where, a removed line as removed", () => {
    const rows = closerRows([
      { kind: "assertion_removed", path: "a/b.test.ts", line: 3, old: true, text: "expect(x).toBe(1)" },
      { kind: "test_deleted", path: "a/c.test.ts" },
    ]);
    expect(rows.map((r) => [r.label, r.place, r.text])).toEqual([
      ["Removed an assertion", "b.test.ts:3 (removed)", "expect(x).toBe(1)"],
      ["Deleted a test file", "c.test.ts", null],
    ]);
  });

  it("says tests, checks or both", () => {
    expect(closerIntro([{ kind: "test_edited", path: "a" }])).toMatch(/^The agent changed tests\./);
    expect(closerIntro([{ kind: "check_silenced", path: "a" }])).toMatch(/^The agent changed checks\./);
  });

  it("marks a check of earlier files, which may run again", () => {
    const stale = { state: "passed", command: "just check", stale: true } as const;
    expect(checkText(stale)).toBe("Check passed, on earlier files");
    expect(canRunAgain(stale)).toBe(true);
    expect(canRunAgain({ state: "passed", command: "x" })).toBe(false);
    expect(canRunAgain({ state: "failed", command: "x" })).toBe(true);
    expect(canRunAgain({ state: "running", command: "x", stale: true })).toBe(false);
    expect(checkText({ state: "running", command: "x", stale: true })).toBe("Check running");
  });
});
