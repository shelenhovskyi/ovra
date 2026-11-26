import { describe, expect, it } from "vitest";
import { claudeCall } from "./claude-calls";
import { describeTool } from "./describe";

const describeClaude = (tool: string, input: unknown) => describeTool(tool, input, claudeCall(tool, input));

describe("describeTool", () => {
  it("says what Ovra's own tools do for a task", () => {
    const action = (name: string, input: unknown) => describeClaude(`mcp__ovra__${name}`, input).action;
    expect(action("spawn_agent", { name: "docs", task: "Write the\ndocs notes." })).toBe(
      "Start worker docs: Write the docs notes.",
    );
    expect(action("request_review", { summary: "Wrote NOTES.md." })).toBe("Ask for review: Wrote NOTES.md.");
    expect(action("request_review", { agent_id: "s2", summary: "ok" })).toBe("Ask for review of worker s2: ok");
    expect(action("read_output", { agent_id: "s3" })).toBe("Read the output of worker s3");
    expect(action("open_in_ui", { path: "src/a.rs", line: 3 })).toBe("Show src/a.rs in Ovra");
    expect(action("list_agents", {})).toBe("List the task's workers");
    expect(describeClaude("mcp__ovra__spawn_agent", { name: "docs", task: "x" }).details).toEqual([
      { label: "task", text: "x" },
    ]);
  });

  it("shows the command a Bash call runs, on one line", () => {
    const summary = describeClaude("Bash", { command: "ls -la\n  && pwd", description: "List files" });
    expect(summary.action).toBe("Run ls -la && pwd");
    expect(summary.details).toEqual([
      { label: "command", text: "ls -la\n  && pwd" },
      { label: "description", text: "List files" },
    ]);
  });

  it("shows the file and both sides of an edit", () => {
    const summary = describeClaude("Edit", { file_path: "README.md", old_string: "teh", new_string: "the" });
    expect(summary.action).toBe("Edit README.md");
    expect(summary.details.map((d) => d.label)).toEqual(["file", "old string", "new string"]);
  });

  it("shows the raw input of tools it does not know, and of known tools with odd input", () => {
    expect(describeClaude("mcp__x__deploy", { env: "prod" })).toEqual({
      action: "Use mcp__x__deploy",
      object: "",
      details: [{ label: "Input", text: '{\n  "env": "prod"\n}' }],
    });
    expect(describeClaude("Bash", { cmd: "rm -rf /" }).details[0]?.text).toContain("rm -rf /");
  });

  it("describes any agent's call by its kind and shared fields, not its name", () => {
    const run = describeTool("run_shell_command", { cmd: "ls" }, { kind: "execute", command: "ls" });
    expect([run.action, run.object]).toEqual(["Run ls", "ls"]);
    const replace = describeTool("replace", {}, { kind: "edit", paths: ["src/a.rs"] });
    expect([replace.action, replace.object]).toEqual(["Edit src/a.rs", "src/a.rs"]);
    const fetch = describeTool("web_fetch", {}, { kind: "fetch", url: "https://docs.rs" });
    expect(fetch.action).toBe("Fetch https://docs.rs");
    // Without shared fields, or saved before them: the tool by name.
    expect(describeTool("Bash", { command: "ls" }, undefined)).toMatchObject({ action: "Use Bash", object: "" });
  });

  it("cuts long one-line actions", () => {
    expect(describeClaude("Bash", { command: "x".repeat(500) }).action.length).toBeLessThanOrEqual(124);
  });
});
