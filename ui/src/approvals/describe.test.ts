import { describe, expect, it } from "vitest";
import { describeTool } from "./describe";

describe("describeTool", () => {
  it("shows the command a Bash call runs, on one line", () => {
    const summary = describeTool("Bash", { command: "ls -la\n  && pwd", description: "List files" });
    expect(summary.action).toBe("Run ls -la && pwd");
    expect(summary.details).toEqual([
      { label: "command", text: "ls -la\n  && pwd" },
      { label: "description", text: "List files" },
    ]);
  });

  it("shows the file and both sides of an edit", () => {
    const summary = describeTool("Edit", { file_path: "README.md", old_string: "teh", new_string: "the" });
    expect(summary.action).toBe("Edit README.md");
    expect(summary.details.map((d) => d.label)).toEqual(["file", "old string", "new string"]);
  });

  it("shows the raw input of tools it does not know, and of known tools with odd input", () => {
    expect(describeTool("mcp__x__deploy", { env: "prod" })).toEqual({
      action: "Use mcp__x__deploy",
      details: [{ label: "Input", text: '{\n  "env": "prod"\n}' }],
    });
    expect(describeTool("Bash", { cmd: "rm -rf /" }).details[0]?.text).toContain("rm -rf /");
  });

  it("cuts long one-line actions", () => {
    expect(describeTool("Bash", { command: "x".repeat(500) }).action.length).toBeLessThanOrEqual(124);
  });
});
