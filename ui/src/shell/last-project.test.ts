import { describe, expect, it } from "vitest";
import type { Project } from "../lib/protocol";
import type { Transport } from "../lib/transport";
import { FakeCore } from "../tree/fake-core";
import { rememberProject, restoreProject } from "./last-project";

const demo: Project = { id: "p1", name: "demo-app", root: "/work/demo-app", last_opened_ms: null, trusted: false };

/** A Storage backed by a map, as the page's localStorage. */
function memoryStorage(): Storage {
  const items = new Map<string, string>();
  return {
    get length() {
      return items.size;
    },
    clear: () => items.clear(),
    getItem: (key) => items.get(key) ?? null,
    key: (index) => [...items.keys()][index] ?? null,
    removeItem: (key) => void items.delete(key),
    setItem: (key, value) => void items.set(key, value),
  };
}

const empty = () => ({ project: null as Project | null, openPath: null as string | null, mode: "view" as const });

describe("last project", () => {
  it("opens the remembered project again", async () => {
    const store = memoryStorage();
    rememberProject("p1", store);
    const core = new FakeCore(undefined, [demo]);
    const workspace = empty();
    await restoreProject(core, workspace, store);
    expect(workspace.project).toEqual(demo);
    expect(core.calls).toEqual([{ method: "projects.open", params: { id: "p1" } }]);
  });

  it("does nothing without one, and forgets it on close", async () => {
    const store = memoryStorage();
    rememberProject("p1", store);
    rememberProject(null, store);
    const core = new FakeCore(undefined, [demo]);
    await restoreProject(core, empty(), store);
    expect(core.calls).toEqual([]);
  });

  it("forgets a project the core no longer knows", async () => {
    const store = memoryStorage();
    rememberProject("gone", store);
    const workspace = empty();
    await restoreProject(new FakeCore(undefined, [demo]), workspace, store);
    expect(workspace.project).toBeNull();
    expect(store.getItem("ovra.project")).toBeNull();
  });

  it("keeps it when the core cannot be reached, and leaves a project opened meanwhile", async () => {
    const store = memoryStorage();
    rememberProject("p1", store);
    const offline = { call: () => Promise.reject(new Error("connection to core closed")) } as unknown as Transport;
    await restoreProject(offline, empty(), store);
    expect(store.getItem("ovra.project")).toBe("p1");

    const other = { ...demo, id: "p2", name: "other" };
    const workspace = { ...empty(), project: other };
    await restoreProject(new FakeCore(undefined, [demo]), workspace, store);
    expect(workspace.project).toBe(other);
  });
});
