import { describe, expect, it } from "vitest";
import { CoreError } from "../lib/transport";
import { FakeAgentCore } from "./fake-core";
import { providers } from "./providers.svelte";

const settle = () => new Promise((resolve) => setImmediate(resolve));

describe("providers", () => {
  it("names a provider from agents.providers, asked once per transport", async () => {
    const core = new FakeAgentCore();
    core.providers = core.providers.map((p) => ({ ...p, name: "Claude (beta)" }));
    const names = providers(core);
    // Until the core answers, the fallback name.
    expect(names.name("claude")).toBe("Claude");
    await settle();
    expect(names.name("claude")).toBe("Claude (beta)");
    expect(providers(core)).toBe(names);
    expect(core.calls.filter((c) => c.method === "agents.providers")).toHaveLength(1);
  });

  it("keeps the fallback name when the core cannot answer", async () => {
    const core = new FakeAgentCore();
    core.failNext.set("agents.providers", new CoreError({ code: "unavailable", message: "offline" }));
    const names = providers(core);
    await settle();
    expect(names.name()).toBe("Claude");
  });
});
