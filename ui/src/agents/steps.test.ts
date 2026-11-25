import { describe, expect, it } from "vitest";
import type { AgentMessage } from "../lib/protocol";
import { elapsed, liveStep } from "./steps";

const use = (id: string, name: string, input: unknown): AgentMessage => ({ kind: "tool_use", id, name, input });
const result = (id: string, is_error = false): AgentMessage => ({
  kind: "tool_result",
  tool_use_id: id,
  output: "ok",
  is_error,
});
const working = { state: "working" } as const;

describe("steps", () => {
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
