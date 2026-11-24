import { describe, expect, it } from "vitest";
import { FakeAgentCore } from "../agents/fake-core";
import { CoreError } from "../lib/transport";
import { ApprovalQueue } from "./queue.svelte";

const bash = (id: string) => ({ id, session: "s1", tool: "Bash", input: { command: "ls" }, title: null });
const tick = () => new Promise((resolve) => setTimeout(resolve));

describe("ApprovalQueue", () => {
  it("starts from the core's pending list and follows events", async () => {
    const core = new FakeAgentCore();
    core.pending = [bash("perm1")];
    const queue = new ApprovalQueue(core);
    await tick();
    expect(queue.pending.map((r) => r.id)).toEqual(["perm1"]);
    core.ask(bash("perm2"));
    core.ask(bash("perm2"));
    expect(queue.pending.map((r) => r.id)).toEqual(["perm1", "perm2"]);
    core.emit({ type: "permissions.resolved", data: { id: "perm1", decision: "deny" } });
    expect(queue.pending.map((r) => r.id)).toEqual(["perm2"]);
  });

  it("sends one answer per request, with the denial message", async () => {
    const core = new FakeAgentCore();
    const queue = new ApprovalQueue(core);
    await tick();
    core.ask(bash("perm1"));
    const [first, second] = await Promise.all([
      queue.answer("perm1", "deny", "use git ls-files"),
      queue.answer("perm1", "allow"),
    ]);
    expect([first, second]).toEqual([true, false]);
    expect(core.sent("permissions.answer").map((c) => c.params)).toEqual([
      { id: "perm1", decision: "deny", message: "use git ls-files" },
    ]);
    expect(queue.pending).toEqual([]);
  });

  it("drops a request answered elsewhere and keeps one whose answer failed", async () => {
    const core = new FakeAgentCore();
    const queue = new ApprovalQueue(core);
    await tick();
    core.ask(bash("perm1"));
    core.failNext.set("permissions.answer", new CoreError({ code: "internal", message: "disk full" }));
    expect(await queue.answer("perm1", "allow")).toBe(false);
    expect(queue.error).toBe("Could not send the answer: disk full");
    expect(queue.pending).toHaveLength(1);
    core.pending = [];
    expect(await queue.answer("perm1", "allow")).toBe(false);
    expect(queue.pending).toEqual([]);
  });
});
