import { describe, expect, it } from "vitest";
import type { AgentMessage } from "../lib/protocol";
import { parseHunks } from "./hunks";
import { fileSteps, stepOf } from "./steps";

const edit = (id: string, path: string, text: string, parent?: string): AgentMessage => ({
  kind: "tool_use",
  id,
  name: "Edit",
  input: { file_path: `/work/a/${path}`, old_string: "x", new_string: text },
  call: { kind: "edit", paths: [`/work/a/${path}`] },
  ...(parent ? { parent } : {}),
});

const messages: AgentMessage[] = [
  { kind: "assistant_text", text: "I'll read the parser first." },
  { kind: "tool_use", id: "t1", name: "Read", input: { file_path: "/work/a/src/p.ts" }, call: { kind: "read" } },
  { kind: "assistant_text", text: "Now add the empty-input check.\nIt returns early." },
  edit("t2", "src/p.ts", "if (!text) return [];"),
  edit("t3", "src/other.ts", "export const n = 1;"),
  edit("t4", "src/p.ts", "never shown", "t9"),
  // A core from before mapped calls: Claude's own names only.
  { kind: "tool_use", id: "t5", name: "Write", input: { file_path: "src/p.ts", content: "const limit = 10;\n" } },
];

const patch = [
  "--- a/src/p.ts",
  "+++ b/src/p.ts",
  "@@ -1,2 +1,3 @@",
  " export function parse(text: string) {",
  "+  if (!text) return [];",
  "   return text.split(',');",
  "@@ -9,2 +10,3 @@",
  " const a = 1;",
  "+const limit = 10;",
  " const b = 2;",
  "@@ -20,2 +22,1 @@",
  " x",
  "-y",
  "",
].join("\n");

describe("stepOf", () => {
  it("names the step whose written text holds a part's added lines", () => {
    const steps = fileSteps(messages, "src/p.ts");
    expect(steps.map((s) => s.number)).toEqual([2, 4]);
    const [first, second, third] = parseHunks(patch);
    if (!first || !second || !third) throw new Error("three parts");
    expect(stepOf(first, steps)).toEqual({ number: 2, tool: "Edit", said: "Now add the empty-input check." });
    expect(stepOf(second, steps)).toEqual({ number: 4, tool: "Write", said: null });
    // Only removed lines, and two steps: unsure.
    expect(stepOf(third, steps)).toBeNull();
    expect(stepOf(third, steps.slice(0, 1))?.number).toBe(2);
  });
});
