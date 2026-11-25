import { describe, expect, it } from "vitest";
import { FakeServersCore } from "./fake-servers";
import { exitDetail, failedStart, ProjectServers, statusText } from "./servers.svelte";

describe("ProjectServers", () => {
  it("reads the list again when someone else starts a server", async () => {
    const core = new FakeServersCore();
    const servers = new ProjectServers(core);
    await servers.show("p1", null);
    expect(servers.list?.servers).toEqual([]);
    await core.call({ method: "servers.start", params: { project: "p1", name: "web" } });
    core.emit({
      type: "servers.status",
      data: { id: "srv1", project: "p1", status: { state: "starting" }, generation: 1 },
    });
    await Promise.resolve();
    await Promise.resolve();
    expect(servers.list?.servers.map((s) => s.id)).toEqual(["srv1"]);
    // Other projects' servers are none of its business.
    const lists = core.calls.filter((c) => c.method === "servers.list").length;
    core.emit({
      type: "servers.status",
      data: { id: "srv9", project: "p2", status: { state: "starting" }, generation: 1 },
    });
    expect(core.calls.filter((c) => c.method === "servers.list")).toHaveLength(lists);
    servers.dispose();
  });

  it("keeps a failed call's reason", async () => {
    const core = new FakeServersCore();
    core.refuse.set("servers.start", { code: "conflict", message: "port 5173 is taken in this pod" });
    const servers = new ProjectServers(core);
    await servers.show("p1", null);
    await servers.start("web");
    expect(servers.error).toBe("port 5173 is taken in this pod");
    expect(servers.unavailable).toBeNull();
    servers.dispose();
  });
});

describe("statusText", () => {
  it("says a failed command in words, keeping the code for the tooltip", () => {
    expect(statusText({ state: "exited", code: 0 })).toBe("exited");
    expect(statusText({ state: "exited", code: 1 })).toBe("stopped: the command failed");
    expect(exitDetail({ state: "exited", code: 1 })).toBe("Exit code 1");
    expect(statusText({ state: "exited", code: null })).toBe("stopped");
    // Asked to stop: whatever code the signal gave is no failure.
    expect(statusText({ state: "exited", code: 143 }, true)).toBe("stopped");
    expect(failedStart({ state: "exited", code: 143 }, true)).toBe(false);
    expect(statusText({ state: "failed", reason: "port 5173 is taken" })).toBe("failed: port 5173 is taken");
    expect(statusText({ state: "stopping" })).toBe("stopping");
    expect(
      [{ state: "exited", code: 1 } as const, { state: "exited", code: null } as const].map((s) => failedStart(s)),
    ).toEqual([true, false]);
  });
});
