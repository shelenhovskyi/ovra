import { describe, expect, it } from "vitest";
import { claudeCall } from "../approvals/claude-calls";
import { describeTool } from "../approvals/describe";
import type { AgentMessage } from "../lib/protocol";
import {
  elapsed,
  entries,
  foldLabel,
  inFolder,
  keepSame,
  liveStep,
  noteLine,
  stepIcon,
  stepLabel,
  stepMeta,
  turnCost,
} from "./steps";

const use = (id: string, name: string, input: unknown): AgentMessage => ({
  kind: "tool_use",
  id,
  name,
  input,
  call: claudeCall(name, input),
});
const result = (id: string, is_error = false): AgentMessage => ({
  kind: "tool_result",
  tool_use_id: id,
  output: "ok",
  is_error,
});
const working = { state: "working" } as const;

describe("steps", () => {
  it("pairs each call with its result and folds only reads and searches in a row", () => {
    const shown = entries([
      { kind: "assistant_text", text: "Looking." },
      use("t1", "Bash", { command: "npm test" }),
      use("t2", "Grep", { pattern: ".ovra/**" }),
      result("t1"),
      result("t2", true),
      { kind: "assistant_text", text: "Done." },
      result("lost"),
    ]);
    // A command and a failed search each keep a card of their own.
    expect(
      shown.map((e) => (e.kind === "steps" ? e.steps.length : e.kind === "message" ? e.message.kind : "notes")),
    ).toEqual(["assistant_text", 1, 1, "assistant_text", 1]);
    const looks = entries([
      use("r1", "Read", { file_path: "a.ts" }),
      use("r2", "Grep", { pattern: "x" }),
      result("r1"),
      use("r3", "Read", { file_path: "b.ts" }),
      use("e1", "Edit", { file_path: "a.ts", old_string: "a", new_string: "b\nc" }),
    ]);
    expect(looks.map((e) => (e.kind === "steps" ? e.steps.length : 0))).toEqual([3, 1]);
    const [fold, edit] = looks;
    expect(fold?.kind === "steps" && foldLabel(fold.steps)).toBe("Read 2 files, searched once");
    expect(edit?.kind === "steps" && edit.steps[0] && stepMeta(edit.steps[0])).toBe("+2 −1");
    // Tokens on a plan, dollars on an API key.
    const usage = { input_tokens: 1200, output_tokens: 300, cost_micro_usd: 84_000 };
    expect(turnCost(usage, { kind: "subscription" })).toBe("1.5k tokens");
    expect(turnCost(usage, { kind: "api_key" })).toBe("$0.08");
    const labels = shown.flatMap((e) =>
      e.kind === "steps"
        ? e.steps.map((s) =>
            stepLabel(s, s.use?.kind === "tool_use" ? describeTool(s.use.name, s.use.input, s.use.call).action : ""),
          )
        : [],
    );
    // A failed call did not do what it says.
    expect(labels).toEqual(["Ran npm test", "Search .ovra/**", "Tool finished"]);
    const spawn = use("t3", "mcp__ovra__spawn_agent", { name: "research-speed", task: "A long task." });
    const [step] = entries([spawn, result("t3")]);
    const action = describeTool(
      "mcp__ovra__spawn_agent",
      { name: "research-speed", task: "A long task." },
      undefined,
    ).action;
    expect(step?.kind === "steps" && stepLabel(step.steps[0] ?? { use: null, result: null }, action)).toBe(
      "Started worker research-speed",
    );
  });

  it("says what a working session does now, this turn only", () => {
    const edit = use("t1", "Edit", { file_path: "src/x.ts", old_string: "a", new_string: "b" });
    expect(liveStep(working, [], false)).toBe("Thinking");
    expect(liveStep(working, [], true)).toBe("Writing");
    expect(liveStep(working, [use("t0", "Bash", { command: "npm test" })], false)).toBe("Running: npm test");
    expect(liveStep(working, [edit], false)).toBe("Editing src/x.ts");
    expect(liveStep(working, [edit, result("t1")], false)).toBe("Thinking");
    // A call an interrupt cut off in an earlier turn runs no more.
    const earlier = [edit, { kind: "turn_end", usage: { input_tokens: 0, output_tokens: 0, cost_micro_usd: null } }];
    expect(liveStep(working, earlier as AgentMessage[], false)).toBe("Thinking");
    expect(liveStep({ state: "starting" }, [], false)).toBe("Starting");
    expect(liveStep({ state: "waiting_for_approval" }, [edit], false)).toBeNull();
    expect(liveStep({ state: "idle" }, [], false)).toBeNull();
    expect([elapsed(4200), elapsed(125_000)]).toEqual(["4s", "2m 05s"]);
  });
});

describe("keepSame", () => {
  it("keeps the earlier entry object where the same messages show, and takes changed ones", () => {
    const text: AgentMessage = { kind: "assistant_text", text: "Looking." };
    const read = use("t1", "Read", { file_path: "/p/a.rs" });
    const note: AgentMessage = { kind: "user_text", text: "Ovra: Files changed", from: "ovra" };
    const before = entries([text, note, read]);
    const after = keepSame(
      before,
      entries([text, note, read, result("t1"), { kind: "assistant_text", text: "Done." }]),
    );
    expect(after[0]).toBe(before[0]);
    expect(after[1]).toBe(before[1]);
    // The read got its result: a new entry, so its row shows it.
    expect(after[2]).not.toBe(before[2]);
    expect(after[2]?.kind === "steps" && after[2].steps[0]?.result?.kind).toBe("tool_result");
    expect(after).toHaveLength(4);
  });
});

describe("any agent's steps", () => {
  it("fold, label and show by the call's kind, whatever the tool is named", () => {
    const call = (id: string, name: string, kind: "read" | "search" | "execute", extra = {}): AgentMessage => ({
      kind: "tool_use",
      id,
      name,
      input: {},
      call: { kind, ...extra },
    });
    const shown = entries([
      call("t1", "read_file", "read", { paths: ["a.md"] }),
      result("t1"),
      call("t2", "glob", "search"),
      result("t2"),
      call("t3", "run_shell_command", "execute", { command: "make" }),
      result("t3"),
    ]);
    expect(shown.map((e) => (e.kind === "steps" ? e.steps.length : 0))).toEqual([2, 1]);
    const [fold, run] = shown;
    expect(fold?.kind === "steps" && foldLabel(fold.steps)).toBe("Read 1 file, searched once");
    expect(run?.kind === "steps" && run.steps[0] && stepIcon(run.steps[0])).toBe("terminal");
    expect(liveStep(working, [call("t4", "run_shell_command", "execute", { command: "make" })], false)).toBe(
      "Running: make",
    );
  });
});

describe("paths in steps", () => {
  it("shows a file in the session's folder relative to it, and others as they are", () => {
    const edits = (...paths: string[]) => ({ kind: "edit" as const, paths });
    expect(inFolder(edits("/tmp/ux/demo-app/src/README.md"), "/tmp/ux/demo-app")).toEqual(edits("src/README.md"));
    expect(inFolder(edits("/wt/docs/a.ipynb", "/wt/docs/b.md"), "/wt/docs/")).toEqual(edits("a.ipynb", "b.md"));
    // Not inside it, a prefix that is not a folder, no folder known, no path.
    expect(inFolder(edits("/etc/hosts"), "/tmp/ux/demo-app")).toEqual(edits("/etc/hosts"));
    expect(inFolder(edits("/tmp/ux/demo-apple/x"), "/tmp/ux/demo-app")).toEqual(edits("/tmp/ux/demo-apple/x"));
    expect(inFolder(edits("/a/b"), null)).toEqual(edits("/a/b"));
    expect(inFolder({ kind: "execute", command: "ls" }, "/a")).toEqual({ kind: "execute", command: "ls" });
    expect(inFolder(undefined, "/a")).toBeUndefined();
    const edit = use("t1", "Edit", { file_path: "/w/src/x.ts", old_string: "a", new_string: "b" });
    expect(liveStep(working, [edit], false, "/w")).toBe("Editing src/x.ts");
  });
});

describe("notes", () => {
  it("says what the person did to a worker's work in Reviews' words", () => {
    // The core's notes to the coordinator, worded as the core words them.
    expect(noteLine("Worker lint had its work approved by the person.")).toBe("You kept lint");
    expect(noteLine("Worker lint had its work rejected by the person; its changes were discarded.")).toBe(
      "You discarded lint",
    );
  });

  it("shows the person's @ context as one line, the message folded under it", () => {
    const text =
      "The person added this context to the message. Read them yourself.\n\nFile: `a.rs`\n\n" +
      'Terminal output from "zsh", printed by commands:\n<untrusted origin="terminal">&lt;b> ok</untrusted>';
    const [shown] = entries([{ kind: "user_text", text, from: "ovra" }]);
    // The wording meant for the agent stays out of the person's view.
    expect(shown).toMatchObject({
      kind: "notes",
      notes: [
        { line: "Context sent", rest: 'File: `a.rs`\n\nTerminal output from "zsh", printed by commands:\n<b> ok' },
      ],
    });
  });
});
