import { describe, expect, it } from "vitest";
import type { AgentSetup } from "../../lib/protocol";
import { programOf, providerState } from "./provider-texts";

const setup = (state: AgentSetup["state"], version: string | null = null): AgentSetup => ({
  path: null,
  executable: "/usr/bin/gemini",
  version,
  state,
  billing: null,
  api_key_in_env: false,
});

describe("provider texts", () => {
  it("says what is wrong with an agent and how its vendor fixes it", () => {
    expect(providerState("gemini", "Gemini", undefined).line).toBe("Checking Gemini…");
    expect(providerState("gemini", "Gemini", setup({ state: "ready" }, "0.71.1")).line).toBe(
      "Gemini is ready (0.71.1). It works only on its own copy.",
    );
    expect(providerState("gemini", "Gemini", setup({ state: "signed_out" }))).toEqual({
      line: "Gemini is not signed in.",
      fix: "Run gemini in a terminal once and sign in with Google, or set GEMINI_API_KEY.",
    });
    expect(providerState("codex", "Codex", setup({ state: "not_found" })).fix).toContain("codex-acp");
    expect(providerState("codex", "Codex", setup({ state: "broken", message: "exit 2" })).line).toBe(
      "Codex could not be checked: exit 2",
    );
    expect(programOf("codex")).toBe("codex-acp");
  });
});
