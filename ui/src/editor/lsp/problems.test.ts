import { expect, test } from "vitest";
import { countProblems, countsLabel, groupByFile, problems, toProblems } from "./problems.svelte";

const at = (line: number, character: number, endLine = line, endCharacter = character + 1) => ({
  start: { line, character },
  end: { line: endLine, character: endCharacter },
});

test("diagnostics become problems: errors first, one-line messages, hints and malformed entries left out", () => {
  const list = toProblems([
    { range: at(4, 0), severity: 3, message: "an info" },
    { range: at(2, 5, 3, 0), severity: 2, message: "spans\n  two lines" },
    { range: at(9, 1), severity: 1, message: "late error" },
    { range: at(0, 0), message: "no severity" },
    { range: at(1, 0), severity: 4, message: "a hint" },
    { range: { start: { line: -1, character: 0 } }, severity: 1, message: "bad range" },
    { range: at(1, 0), severity: 1 },
  ]);
  expect(list.map((p) => [p.severity, p.message, `${p.line}:${p.column}`])).toEqual([
    ["error", "no severity", "1:1"],
    ["error", "late error", "10:2"],
    ["warning", "spans two lines", "3:6"],
    ["info", "an info", "5:1"],
  ]);
  // A range over several lines selects only its start.
  expect(list[2]?.spot).toEqual({ line: 3, from: 5, to: 5 });
  expect(toProblems(null)).toEqual([]);
});

test("files with errors come first, then by path; servers' problems for one file merge", () => {
  const warning = toProblems([{ range: at(0, 0), severity: 2, message: "w" }]);
  const error = toProblems([{ range: at(5, 0), severity: 1, message: "e" }]);
  const files = groupByFile({
    s1: { "b.ts": warning, "c.ts": warning, "z.ts": error },
    s2: { "c.ts": error, "a.ts": [] },
  });
  expect(files.map((f) => [f.path, f.problems.map((p) => p.message)])).toEqual([
    ["c.ts", ["e", "w"]],
    ["z.ts", ["e"]],
    ["b.ts", ["w"]],
  ]);
  const counts = countProblems(files);
  expect(counts).toEqual({ error: 2, warning: 2, info: 0 });
  expect(countsLabel(counts)).toBe("2 errors, 2 warnings");
  expect(countsLabel({ error: 1, warning: 0, info: 3 })).toBe("1 error");
  expect(countsLabel({ error: 0, warning: 0, info: 3 })).toBe("");
});

test("a new list for a file replaces the old one; an empty list or a gone server clears it", () => {
  const diagnostics = (message: string) => [{ range: at(0, 0), severity: 1, message }];
  problems.publish("p9", "s1", "a.ts", diagnostics("old"));
  problems.publish("p9", "s1", "a.ts", diagnostics("new"));
  problems.publish("p9", "s1", "b.ts", diagnostics("b"));
  problems.publish("p8", "s3", "a.ts", diagnostics("other project"));
  expect(problems.files("p9").map((f) => [f.path, f.problems[0]?.message])).toEqual([
    ["a.ts", "new"],
    ["b.ts", "b"],
  ]);
  problems.publish("p9", "s1", "a.ts", []);
  expect(problems.files("p9").map((f) => f.path)).toEqual(["b.ts"]);
  problems.forget("p9", "s1");
  expect(problems.files("p9")).toEqual([]);
  expect(problems.files("p8")).toHaveLength(1);
  expect(problems.files(null)).toEqual([]);
});
