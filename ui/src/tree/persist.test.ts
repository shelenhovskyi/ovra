import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { FakeCore } from "./fake-core";
import { ExpandedSaver, loadExpanded } from "./persist";

beforeEach(() => {
  vi.useFakeTimers();
});
afterEach(() => {
  vi.useRealTimers();
});

const saves = (core: FakeCore) => core.calls.filter((c) => c.method === "projects.set_state").length;

describe("ExpandedSaver", () => {
  it("saves once after changes settle and keeps the rest of the state", async () => {
    const core = new FakeCore();
    core.states.set("p1", { open_files: ["a.ts"], active_file: "a.ts", expanded_dirs: [] });
    const saver = new ExpandedSaver(core, "p1", [], 500);
    saver.update(["src"]);
    saver.update(["src", "src/lib"]);
    await vi.advanceTimersByTimeAsync(499);
    expect(saves(core)).toBe(0);
    await vi.advanceTimersByTimeAsync(1);
    expect(saves(core)).toBe(1);
    // One partial update, no read first.
    expect(core.calls.map((c) => c.method)).toEqual(["projects.set_state"]);
    expect(core.states.get("p1")).toEqual({
      open_files: ["a.ts"],
      active_file: "a.ts",
      expanded_dirs: ["src", "src/lib"],
    });
  });

  it("skips saving what is already saved and flushes on demand", async () => {
    const core = new FakeCore();
    const saver = new ExpandedSaver(core, "p1", ["src"], 500);
    saver.update(["src"]);
    await vi.runAllTimersAsync();
    expect(saves(core)).toBe(0);
    saver.update([]);
    await saver.flush();
    expect(core.states.get("p1")?.expanded_dirs).toEqual([]);
  });

  it("loads saved folders, or none when the state is unavailable", async () => {
    const core = new FakeCore();
    core.states.set("p1", { open_files: [], active_file: null, expanded_dirs: ["src"] });
    await expect(loadExpanded(core, "p1")).resolves.toEqual(["src"]);
    const broken = { call: () => Promise.reject(new Error("unavailable")), onEvent: () => () => {} };
    await expect(loadExpanded(broken, "p1")).resolves.toEqual([]);
  });
});
