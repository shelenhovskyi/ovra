import { describe, expect, it } from "vitest";
import type { AgentMessage } from "../lib/protocol";
import { failedLabel, isAuthFailure, signedOutError, withoutEchoes } from "./errors";

const AUTH = "Failed to authenticate: OAuth session expired and could not be refreshed";
const you = (text: string): AgentMessage => ({ kind: "user_text", text, from: "person" });
const claude = (text: string): AgentMessage => ({ kind: "assistant_text", text });
const error = (message: string): AgentMessage => ({ kind: "error", message });
const end: AgentMessage = { kind: "turn_end", usage: { input_tokens: 0, output_tokens: 0, cost_micro_usd: null } };

describe("session errors", () => {
  it("tell a signed-out account from other failures", () => {
    expect(isAuthFailure(AUTH)).toBe(true);
    expect(isAuthFailure("Invalid API key · Please run /login")).toBe(true);
    expect(isAuthFailure("The turn stopped: it reached its budget.")).toBe(false);
  });

  it("say a turn that ended with an error failed, not done", () => {
    const idle = { state: "idle" } as const;
    expect(failedLabel(idle, [you("Fix it"), claude(AUTH), error(AUTH), end])).toBe("Failed: signed out");
    expect(failedLabel({ state: "stopped" }, [you("Fix it"), error(AUTH), end])).toBe("Failed: signed out");
    expect(failedLabel(idle, [you("Fix it"), error("The turn failed."), end])).toBe("Failed");
    // An interrupt, or an error of an older turn, is no failure of this one.
    expect(failedLabel(idle, [you("Fix it"), error("Interrupted."), end])).toBeNull();
    expect(failedLabel(idle, [you("a"), error(AUTH), end, you("b"), claude("Done."), end])).toBeNull();
    expect(signedOutError({ state: "failed", message: AUTH }, [])).toBe(AUTH);
    expect(signedOutError({ state: "working" }, [you("a"), error(AUTH)])).toBeNull();
  });

  it("drop Claude's text that only repeats its turn's error", () => {
    const messages = [you("a"), claude(AUTH), error(AUTH), end, you("b"), claude(AUTH), end];
    expect(withoutEchoes(messages)).toEqual([you("a"), error(AUTH), end, you("b"), claude(AUTH), end]);
  });
});
