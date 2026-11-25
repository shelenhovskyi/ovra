import { describe, expect, it } from "vitest";
import { FakePolicyCore } from "./fake-policy";
import { ChangedElsewhere, ProjectPolicy } from "./policy.svelte";

const settle = async () => {
  for (let i = 0; i < 4; i++) await Promise.resolve();
};

describe("ProjectPolicy", () => {
  it("reads the project's rules and reads them again when they change elsewhere", async () => {
    const core = new FakePolicyCore();
    core.rules.set("p1", [{ tool: "WebFetch", action: "deny" }]);
    const policy = new ProjectPolicy(core);
    await policy.show("p1");
    expect(policy.rules).toEqual([{ tool: "WebFetch", action: "deny" }]);

    core.rules.set("p1", []);
    core.emit({ type: "policy.changed", data: { project: "p2" } });
    await settle();
    expect(policy.rules).toHaveLength(1);
    core.emit({ type: "policy.changed", data: { project: "p1" } });
    await settle();
    expect(policy.rules).toEqual([]);
    policy.dispose();
  });

  it("saves through policy.set and passes the core's refusal on in its own words", async () => {
    const core = new FakePolicyCore();
    core.refuse.set("Grep", "Grep(x): only Bash and file tools take a match");
    const policy = new ProjectPolicy(core);
    await policy.show("p1");
    await policy.save([{ tool: "Bash", match: "npm test", action: "auto" }], []);
    expect(policy.rules).toEqual([{ tool: "Bash", match: "npm test", action: "auto" }]);
    expect(core.calls.at(-2)).toMatchObject({ method: "policy.set", params: { expected: { rules: [] } } });
    await expect(policy.save([{ tool: "Grep", match: "x", action: "deny" }], policy.rules ?? [])).rejects.toThrow(
      /Grep\(x\)/,
    );
    expect(policy.rules).toHaveLength(1);
    policy.dispose();
  });

  it("refuses a change built from an older policy and reads the fresh one", async () => {
    const core = new FakePolicyCore();
    const policy = new ProjectPolicy(core);
    await policy.show("p1");
    // Another window adds a deny rule; its event has not arrived yet.
    core.rules.set("p1", [{ tool: "WebFetch", action: "deny" }]);
    await expect(policy.save([{ tool: "Bash", action: "ask" }], [])).rejects.toBeInstanceOf(ChangedElsewhere);
    expect(core.rules.get("p1")).toEqual([{ tool: "WebFetch", action: "deny" }]);
    expect(policy.rules).toEqual([{ tool: "WebFetch", action: "deny" }]);
    policy.dispose();
  });

  it("drops a saved reply when a read started after the change was sent", async () => {
    const core = new FakePolicyCore();
    const policy = new ProjectPolicy(core);
    await policy.show("p1");
    core.hold = "policy.set";
    const saving = policy.save([{ tool: "Bash", action: "ask" }], []);
    await settle();
    // Another window changes the policy after ours was kept; that read is newer.
    core.hold = null;
    core.rules.set("p1", [{ tool: "WebFetch", action: "deny" }]);
    core.emit({ type: "policy.changed", data: { project: "p1" } });
    await settle();
    core.release();
    await saving;
    expect(policy.rules).toEqual([{ tool: "WebFetch", action: "deny" }]);
    policy.dispose();
  });

  it("reads the rules again after a reconnect", async () => {
    const core = new FakePolicyCore();
    const policy = new ProjectPolicy(core);
    core.setStatus("open");
    await policy.show("p1");
    core.rules.set("p1", [{ tool: "WebFetch", action: "deny" }]);
    core.setStatus("closed");
    core.setStatus("open");
    await settle();
    expect(policy.rules).toEqual([{ tool: "WebFetch", action: "deny" }]);
    expect(core.calls.filter((c) => c.method === "policy.get")).toHaveLength(2);
    policy.dispose();
  });
});
