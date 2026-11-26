import { describe, expect, it } from "vitest";
import { claudeCall } from "./claude-calls";
import { describeTool, pushScope } from "./describe";

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

describe("a worker's push", () => {
  const plan = {
    remote: "origin",
    host: "github.com",
    branch: "ovra/t1/w",
    new_branch: true,
    base: "main",
    onto: "c1",
    commit: "c2",
    message: "Add b\n\nThe second file.",
    files: [
      { path: "b.rs", kind: "added", additions: 1, deletions: 0 },
      { path: "c.rs", kind: "renamed", from: "a.rs", additions: 2, deletions: 1 },
    ],
    protected_branch: false,
  };

  it("says where it goes, what it builds on, its files and message", () => {
    const summary = describeTool("Push", plan, { kind: "other" });
    expect([summary.action, summary.object]).toEqual(["Push to origin/ovra/t1/w", "origin/ovra/t1/w"]);
    expect(summary.details).toEqual([
      { label: "where", text: "A new branch from origin/main on github.com" },
      { label: "2 files", text: "A b.rs  +1 −0\nR a.rs → c.rs  +2 −1" },
      { label: "commit message", text: "Add b\n\nThe second file." },
    ]);
    const shared = describeTool(
      "Push",
      { ...plan, branch: "main", new_branch: false, protected_branch: true },
      undefined,
    );
    expect(shared.details.slice(0, 2).map((d) => d.label)).toEqual(["where", "shared branch"]);
    expect(shared.details[0]?.text).toBe("One new commit on top of origin/main on github.com");
    // Input of another shape shows raw, as for any tool.
    expect(describeTool("Push", { remote: "origin" }, undefined).action).toBe("Use Push");
  });

  it("puts a push rule in words", () => {
    expect(pushScope("origin ovra/*")).toEqual({ remote: "origin", branches: "ovra/…" });
    expect(pushScope("up fix-login")).toEqual({ remote: "up", branches: "fix-login" });
    expect(pushScope("origin")).toBeNull();
  });
});
