import { describe, expect, it } from "vitest";
import { cardBody } from "./body";
import { claudeCall } from "./claude-calls";
import { describeTool } from "./describe";

const body = (tool: string, input: unknown) => {
  const call = claudeCall(tool, input);
  return cardBody(input, call, describeTool(tool, input, call));
};

describe("cardBody", () => {
  it("shows a command once and its description as plain text", () => {
    expect(body("Bash", { command: "ls -la", description: "List files" })).toEqual([
      { kind: "note", text: "List files" },
    ]);
    // The title puts it on one line; the whole command is still shown.
    expect(body("Bash", { command: "ls\n&& rm -rf /" })).toEqual([{ kind: "code", text: "ls\n&& rm -rf /" }]);
    const long = `echo ${"x".repeat(300)}`;
    expect(body("Bash", { command: long })).toEqual([{ kind: "code", text: long }]);
  });

  it("shows an edit as a diff, without naming the file again", () => {
    expect(body("Edit", { file_path: "/demo/README.md", old_string: "teh", new_string: "the" })).toEqual([
      { kind: "diff", removed: ["teh"], added: ["the"] },
    ]);
    expect(
      body("MultiEdit", {
        file_path: "/demo/a.ts",
        edits: [
          { old_string: "a\nb", new_string: "c" },
          { old_string: "x", new_string: "", replace_all: true },
        ],
      }),
    ).toEqual([
      { kind: "diff", removed: ["a", "b"], added: ["c"] },
      { kind: "diff", removed: ["x"], added: [] },
      { kind: "note", text: "Replaces every match in the file." },
    ]);
  });

  it("keeps the raw input when an edit is not a plain replacement", () => {
    expect(body("NotebookEdit", { notebook_path: "/n.ipynb", cell_id: "c1", edit_mode: "delete" })).toEqual([
      { kind: "fields", fields: [{ label: "Input", text: expect.stringContaining('"edit_mode": "delete"') }] },
    ]);
    expect(body("Edit", { file_path: "/demo/a" })).toEqual([
      { kind: "fields", fields: [{ label: "Input", text: '{\n  "file_path": "/demo/a"\n}' }] },
    ]);
  });

  it("previews what a write puts in the file, and drops fields the title says", () => {
    expect(body("Write", { file_path: "NOTES.md", content: "Notes." })).toEqual([{ kind: "code", text: "Notes." }]);
    expect(body("Read", { file_path: "/demo/a" })).toEqual([]);
    expect(body("Grep", { pattern: "TODO", path: "s" })).toEqual([
      { kind: "fields", fields: [{ label: "Path", text: "s" }] },
    ]);
  });
});
