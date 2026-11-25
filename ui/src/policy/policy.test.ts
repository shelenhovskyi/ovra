import { describe, expect, it } from "vitest";
import { FakePolicyCore } from "./fake-policy";
import { ProjectPolicy } from "./policy.svelte";

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
    await policy.save([{ tool: "Bash", match: "npm test", action: "auto" }]);
    expect(policy.rules).toEqual([{ tool: "Bash", match: "npm test", action: "auto" }]);
    await expect(policy.save([{ tool: "Grep", match: "x", action: "deny" }])).rejects.toThrow(/Grep\(x\)/);
    expect(policy.rules).toHaveLength(1);
    policy.dispose();
  });
});
