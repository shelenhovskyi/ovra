import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import FileTree from "./FileTree.svelte";
import { dirsFromPaths, FakeCore, flatDir } from "./fake-core";
import ProjectList from "./ProjectList.svelte";

// jsdom has no ResizeObserver, which `bind:clientHeight` uses; a viewport of 0 px still renders the overscan rows.
globalThis.ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

let cleanup: (() => void) | null = null;
afterEach(() => {
  cleanup?.();
  cleanup = null;
  document.body.innerHTML = "";
});

/** Lets pending fake calls resolve and Svelte re-render. */
async function settle(): Promise<void> {
  flushSync();
  await new Promise((resolve) => setTimeout(resolve));
  flushSync();
}

function mountTree(core: FakeCore) {
  const opened: string[] = [];
  const target = document.body.appendChild(document.createElement("div"));
  const app = mount(FileTree, { target, props: { transport: core, project: "p1", onopen: (p) => opened.push(p) } });
  cleanup = () => unmount(app);
  const tree = () => target.querySelector<HTMLElement>("[role=tree]") as HTMLElement;
  const rows = () => [...target.querySelectorAll<HTMLElement>("[role=treeitem]")];
  const row = (path: string) => target.querySelector<HTMLElement>(`[data-path="${path}"]`) as HTMLElement;
  const key = (k: string) => {
    tree().dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true }));
    flushSync();
  };
  return { opened, tree, rows, row, key };
}

describe("FileTree", () => {
  it("expands directories on click and opens files", async () => {
    const t = mountTree(new FakeCore(dirsFromPaths(["src/a.ts", "README.md"], { "src/a.ts": "added" })));
    await settle();
    expect(t.rows().map((r) => r.dataset.path)).toEqual(["src", "README.md"]);

    t.row("src").click();
    await settle();
    expect(t.row("src").getAttribute("aria-expanded")).toBe("true");
    expect(t.row("src/a.ts").textContent).toContain("A");

    t.row("src/a.ts").click();
    expect(t.opened).toEqual(["src/a.ts"]);
  });

  it("moves with the keyboard: down, right to expand, enter to open, left to parent", async () => {
    const t = mountTree(new FakeCore(dirsFromPaths(["src/a.ts", "z.md"])));
    await settle();
    t.key("ArrowDown");
    expect(t.row("src").getAttribute("aria-selected")).toBe("true");
    t.key("ArrowRight");
    await settle();
    t.key("ArrowRight");
    expect(t.row("src/a.ts").getAttribute("aria-selected")).toBe("true");
    t.key("Enter");
    expect(t.opened).toEqual(["src/a.ts"]);
    t.key("ArrowLeft");
    expect(t.row("src").getAttribute("aria-selected")).toBe("true");
    t.key("ArrowLeft");
    expect(t.row("src").getAttribute("aria-expanded")).toBe("false");
    t.key("End");
    expect(t.row("z.md").getAttribute("aria-selected")).toBe("true");
  });

  it("renders only a window of rows for 100k files", async () => {
    const t = mountTree(new FakeCore(new Map([["", flatDir(100_000)]])));
    await settle();
    expect(t.rows().length).toBeLessThan(60);
    expect(t.rows()[0]?.getAttribute("aria-setsize")).toBe("100000");

    Object.defineProperty(t.tree(), "scrollTop", { value: 22 * 50_000, configurable: true });
    t.tree().dispatchEvent(new Event("scroll"));
    flushSync();
    const shown = t.rows().map((r) => r.dataset.path);
    expect(shown).toContain("f050000");
    expect(shown.length).toBeLessThan(60);
  });

  it("reloads on fs.changed and fs.git_changed for its project only", async () => {
    const core = new FakeCore(dirsFromPaths(["a.ts"]));
    const t = mountTree(core);
    await settle();
    core.dirs = dirsFromPaths(["a.ts", "b.ts"], { "a.ts": "modified" });
    core.emit({ type: "fs.changed", data: { project: "other", paths: ["b.ts"] } });
    await settle();
    expect(t.rows()).toHaveLength(1);
    core.emit({ type: "fs.changed", data: { project: "p1", paths: ["b.ts"] } });
    await settle();
    expect(t.rows().map((r) => r.dataset.path)).toEqual(["a.ts", "b.ts"]);
    expect(t.row("a.ts").textContent).toContain("M");
  });

  it("restores the folders expanded last time and saves new ones", async () => {
    const core = new FakeCore(dirsFromPaths(["src/lib/b.ts", "docs/a.md"]));
    core.states.set("p1", { open_files: [], active_file: null, expanded_dirs: ["src", "src/lib"] });
    const t = mountTree(core);
    await settle();
    expect(t.rows().map((r) => r.dataset.path)).toEqual(["docs", "src", "src/lib", "src/lib/b.ts"]);
    t.row("docs").click();
    await settle();
    cleanup?.(); // closing the tree flushes the pending save
    cleanup = null;
    await settle();
    expect(core.states.get("p1")?.expanded_dirs).toEqual(["docs", "src", "src/lib"]);
  });

  it("shows why the tree could not load", async () => {
    const core = new FakeCore(new Map());
    const target = document.body.appendChild(document.createElement("div"));
    const app = mount(FileTree, { target, props: { transport: core, project: "p1", onopen: () => {} } });
    cleanup = () => unmount(app);
    await settle();
    expect(target.querySelector("[role=alert]")?.textContent).toBe("not_found: no such directory: ");
  });
});

describe("ProjectList", () => {
  it("lists projects and opens one", async () => {
    const core = new FakeCore(undefined, [{ id: "p1", name: "ovra", root: "/src/ovra", last_opened_ms: null }]);
    const opened: string[] = [];
    const target = document.body.appendChild(document.createElement("div"));
    const app = mount(ProjectList, { target, props: { transport: core, onopen: (p) => opened.push(p.id) } });
    cleanup = () => unmount(app);
    await settle();
    target.querySelector<HTMLButtonElement>("li button")?.click();
    await settle();
    expect(opened).toEqual(["p1"]);
  });

  it("adds a project by folder and opens it", async () => {
    const core = new FakeCore();
    const opened: string[] = [];
    const target = document.body.appendChild(document.createElement("div"));
    const app = mount(ProjectList, { target, props: { transport: core, onopen: (p) => opened.push(p.name) } });
    cleanup = () => unmount(app);
    await settle();
    expect(target.textContent).toContain("No projects yet.");
    const input = target.querySelector("input") as HTMLInputElement;
    input.value = "/home/me/demo";
    input.dispatchEvent(new Event("input"));
    target.querySelector("form")?.requestSubmit();
    await settle();
    expect(opened).toEqual(["demo"]);
  });
});
