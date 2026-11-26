import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import { listCommands } from "../keys/commands";
import type { WindowDrop } from "../lib/transport";
import { Workspace } from "../shell/workspace.svelte";
import { FakeCore } from "./fake-core";
import TreeHarness from "./TreeHarness.svelte";

// jsdom has no ResizeObserver, which the file tree's `bind:clientHeight` uses.
globalThis.ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

/** A fake core in the desktop window: a folder dialog and dropped paths. */
class DesktopCore extends FakeCore {
  picked: string | null = "/work/picked";
  private drops = new Set<(drop: WindowDrop) => void>();
  async pickFolder(): Promise<string | null> {
    return this.picked;
  }
  onDrop(listener: (drop: WindowDrop) => void): () => void {
    this.drops.add(listener);
    return () => this.drops.delete(listener);
  }
  drop(paths: string[]): void {
    for (const listener of this.drops) listener({ id: "d", paths, x: 0, y: 0 });
  }
}

let cleanup: (() => void) | null = null;
afterEach(() => {
  cleanup?.();
  cleanup = null;
  document.body.innerHTML = "";
});

async function settle(): Promise<void> {
  for (let i = 0; i < 3; i++) {
    flushSync();
    await new Promise((resolve) => setImmediate(resolve));
  }
  flushSync();
}

function mountTree(core: FakeCore) {
  const workspace = new Workspace();
  const target = document.body.appendChild(document.createElement("div"));
  const app = mount(TreeHarness, { target, props: { ovra: { transport: core, workspace } } });
  cleanup = () => unmount(app);
  return { workspace, target };
}

const openFolder = () => listCommands().find((c) => c.id === "tree.open_folder");

describe("Tree: opening a folder", () => {
  it("registers Open folder… on Mod+O and opens the picked folder", async () => {
    const core = new DesktopCore();
    const { workspace } = mountTree(core);
    await settle();
    expect(openFolder()).toMatchObject({ title: "Open folder…", keys: "Mod+O" });
    await openFolder()?.run();
    await settle();
    expect(workspace.project?.root).toBe("/work/picked");
  });

  it("has no Open folder command without the system dialog", async () => {
    mountTree(new FakeCore());
    await settle();
    expect(openFolder()).toBeUndefined();
  });

  it("opens a folder dropped on the start screen and ignores drops over a project", async () => {
    const core = new DesktopCore();
    const { workspace } = mountTree(core);
    await settle();
    core.drop(["/work/one"]);
    await settle();
    expect(workspace.project?.root).toBe("/work/one");
    workspace.openPath = "a.ts";
    core.drop(["/work/two"]);
    await settle();
    expect(workspace.project?.root).toBe("/work/one");
    expect(workspace.openPath).toBe("a.ts");
  });

  it("shows a failed Mod+O over a project in the files view and drops it on close", async () => {
    const core = new DesktopCore();
    const { workspace, target } = mountTree(core);
    await settle();
    await openFolder()?.run();
    await settle();
    core.picked = "relative";
    workspace.sidebar = "search";
    await openFolder()?.run();
    await settle();
    expect(workspace.project?.root).toBe("/work/picked");
    expect(workspace.sidebar).toBe("files");
    expect(target.querySelector("[role=alert]")?.textContent).toContain("Use the full path");
    workspace.project = null;
    await settle();
    expect(target.querySelector("[role=alert]")).toBeNull();
  });

  it("brings back the files view and focuses the tree on Show files", async () => {
    const core = new DesktopCore();
    const { workspace, target } = mountTree(core);
    await settle();
    expect(listCommands().some((c) => c.id === "tree.focus")).toBe(false);
    await openFolder()?.run();
    await settle();
    workspace.sidebar = "git";
    const show = listCommands().find((c) => c.id === "tree.focus");
    expect(show?.keys).toBe("Mod+Shift+E");
    await show?.run();
    expect(workspace.sidebar).toBe("files");
    expect(document.activeElement).toBe(target.querySelector("[role=tree]"));
  });
});
