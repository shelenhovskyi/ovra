import { describe, expect, it } from "vitest";
import { describeTool } from "../approvals/describe";
import type { AgentMessage } from "../lib/protocol";
import { elapsed, entries, foldLabel, liveStep, stepLabel, stepMeta, turnCost } from "./steps";

const use = (id: string, name: string, input: unknown): AgentMessage => ({ kind: "tool_use", id, name, input });
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
    expect(shown.map((e) => (e.kind === "message" ? e.message.kind : e.steps.length))).toEqual([
      "assistant_text",
      1,
      1,
      "assistant_text",
      1,
    ]);
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
            stepLabel(s, s.use?.kind === "tool_use" ? describeTool(s.use.name, s.use.input).action : ""),
          )
        : [],
    );
    // A failed call did not do what it says.
    expect(labels).toEqual(["Ran npm test", "Search .ovra/**", "Tool finished"]);
    const spawn = use("t3", "mcp__ovra__spawn_agent", { name: "research-speed", task: "A long task." });
    const [step] = entries([spawn, result("t3")]);
    const action = describeTool("mcp__ovra__spawn_agent", { name: "research-speed", task: "A long task." }).action;
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
