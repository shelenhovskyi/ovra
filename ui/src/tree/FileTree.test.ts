import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import { listCommands } from "../keys/commands";
import FileTree from "./FileTree.svelte";
import { dirsFromPaths, FakeCore, flatDir } from "./fake-core";
import { FolderOpener } from "./folder-opener.svelte";
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
  // A few rounds: restored folders load level by level as their rows appear.
  for (let i = 0; i < 3; i++) {
    flushSync();
    await new Promise((resolve) => setImmediate(resolve));
  }
  flushSync();
}

function mountTree(core: FakeCore, openPath: string | null = null) {
  const opened: string[] = [];
  const kept: string[] = [];
  const target = document.body.appendChild(document.createElement("div"));
  const app = mount(FileTree, {
    target,
    props: { transport: core, project: "p1", openPath, onopen: (p) => opened.push(p), onkeep: (p) => kept.push(p) },
  });
  cleanup = () => unmount(app);
  const tree = () => target.querySelector<HTMLElement>("[role=tree]") as HTMLElement;
  const rows = () => [...target.querySelectorAll<HTMLElement>("[role=treeitem]")];
  const row = (path: string) => target.querySelector<HTMLElement>(`[data-path="${path}"]`) as HTMLElement;
  const key = (k: string) => {
    tree().dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true }));
    flushSync();
  };
  return { opened, kept, tree, rows, row, key };
}

describe("FileTree", () => {
  it("expands directories on click and opens files", async () => {
    const t = mountTree(new FakeCore(dirsFromPaths(["src/a.ts", "README.md"], { "src/a.ts": "added" })));
    await settle();
    expect(t.rows().map((r) => r.dataset.path)).toEqual(["src", "README.md"]);

    t.row("src").click();
    await settle();
    expect(t.row("src").getAttribute("aria-expanded")).toBe("true");
    expect(t.row("src/a.ts").querySelector(".dot")?.getAttribute("title")).toBe("A: added");

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

  it("renders only a window of rows for 20k files", async () => {
    const t = mountTree(new FakeCore(new Map([["", flatDir(20_000)]])));
    await settle();
    expect(t.rows().length).toBeLessThan(60);
    expect(t.rows()[0]?.getAttribute("aria-setsize")).toBe("20000");

    Object.defineProperty(t.tree(), "scrollTop", { value: 28 * 10_000, configurable: true });
    t.tree().dispatchEvent(new Event("scroll"));
    flushSync();
    const shown = t.rows().map((r) => r.dataset.path);
    expect(shown).toContain("f010000");
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
    // The status is a dot with its letter in the tooltip, and a word for screen readers.
    expect(t.row("a.ts").querySelector(".dot")?.getAttribute("title")).toBe("M: modified");
    expect(t.row("a.ts").textContent).toContain("modified");
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

  it("names the active row through aria-activedescendant, even when it scrolls out of the window", async () => {
    const t = mountTree(new FakeCore(new Map([["", flatDir(1000)]])));
    await settle();
    expect(document.activeElement).toBe(t.tree());
    t.key("ArrowDown");
    const active = () => document.getElementById(t.tree().getAttribute("aria-activedescendant") ?? "");
    expect(active()?.dataset.path).toBe("f000000");
    Object.defineProperty(t.tree(), "scrollTop", { value: 28 * 500, configurable: true, writable: true });
    t.tree().dispatchEvent(new Event("scroll"));
    flushSync();
    expect(active()?.dataset.path).toBe("f000000");
    expect(active()?.getAttribute("aria-selected")).toBe("true");
  });

  it("gives rows clean names, per-folder positions and status as text and attributes", async () => {
    const dirs = dirsFromPaths(["src/a.ts", "src/b.ts", "dist/x.js"], { src: "modified", "src/a.ts": "modified" });
    const dist = dirs.get("")?.find((e) => e.name === "dist");
    if (dist) dist.ignored = true;
    const t = mountTree(new FakeCore(dirs));
    await settle();
    t.row("src").click();
    await settle();
    const description = (el: HTMLElement) =>
      document.getElementById(el.getAttribute("aria-describedby") ?? "")?.textContent ?? null;
    const a = t.row("src/a.ts");
    expect(a.getAttribute("aria-label")).toBe("a.ts");
    expect(description(a)).toBe("modified");
    expect(a.dataset.git).toBe("modified");
    expect([a.getAttribute("aria-posinset"), a.getAttribute("aria-setsize")]).toEqual(["1", "2"]);
    expect(description(t.row("src"))).toBe("contains modified files");
    expect(t.row("dist").dataset.ignored).toBe("true");
    expect(description(t.row("dist"))).toBe("ignored");
  });

  it("shows deleted files only when changed, struck through and unopenable, and where a renamed file came from", async () => {
    const dirs = dirsFromPaths(["gone.ts", "new.ts"], { "gone.ts": "deleted", "new.ts": "renamed" });
    const renamed = dirs.get("")?.find((e) => e.name === "new.ts");
    if (renamed) renamed.renamed_from = "old/name.ts";
    const t = mountTree(new FakeCore(dirs));
    await settle();
    expect(document.querySelector('[data-path="gone.ts"]')).toBeNull();
    document.querySelector<HTMLButtonElement>('[aria-label="Changed only"]')?.click();
    await settle();
    const description = (el: HTMLElement) =>
      document.getElementById(el.getAttribute("aria-describedby") ?? "")?.textContent ?? null;
    expect(description(t.row("gone.ts"))).toBe("deleted");
    expect(t.row("gone.ts").querySelector(".name.git-deleted")).not.toBeNull();
    t.row("gone.ts").click();
    expect(t.opened).toEqual([]);
    expect(description(t.row("new.ts"))).toBe("renamed, from old/name.ts");
    // The letter in the tooltip, besides the dot's colour.
    expect(t.row("new.ts").title).toBe("new.ts (R: renamed, from old/name.ts)");
    expect(t.row("gone.ts").title).toBe("gone.ts (D: deleted)");
  });

  it("keeps a selection when its row goes away: parent on collapse, neighbour on delete", async () => {
    const core = new FakeCore(dirsFromPaths(["src/a.ts", "src/b.ts", "src/c.ts"]));
    const t = mountTree(core);
    await settle();
    t.row("src").click();
    await settle();
    t.key("ArrowDown");
    t.key("ArrowDown");
    expect(t.row("src/b.ts").getAttribute("aria-selected")).toBe("true");
    core.dirs = dirsFromPaths(["src/a.ts", "src/c.ts"]);
    core.emit({ type: "fs.changed", data: { project: "p1", paths: ["src/b.ts"] } });
    await settle();
    expect(t.row("src/c.ts").getAttribute("aria-selected")).toBe("true");
    [...document.querySelectorAll("button")].find((b) => b.getAttribute("aria-label") === "Collapse all")?.click();
    await settle();
    expect(t.row("src").getAttribute("aria-selected")).toBe("true");
    expect(t.rows().map((r) => r.dataset.path)).toEqual(["src"]);
  });

  it("pages, and jumps by typed prefix", async () => {
    const t = mountTree(new FakeCore(dirsFromPaths(["apple.ts", "banana.ts", "blueberry.ts", "cherry.ts"])));
    await settle();
    t.key("b");
    expect(t.row("banana.ts").getAttribute("aria-selected")).toBe("true");
    t.key("l");
    expect(t.row("blueberry.ts").getAttribute("aria-selected")).toBe("true");
    t.key("PageDown");
    expect(t.row("cherry.ts").getAttribute("aria-selected")).toBe("true");
  });

  it("starts a new typed prefix after Enter or any other key that types nothing", async () => {
    const t = mountTree(new FakeCore(dirsFromPaths(["Cargo.toml", "README.md", "src.rs"])));
    await settle();
    for (const k of ["c", "a", "r", "Enter", "r", "e"]) t.key(k);
    expect(t.opened).toEqual(["Cargo.toml"]);
    expect(t.row("README.md").getAttribute("aria-selected")).toBe("true");
    for (const k of ["s", "ArrowUp", "c"]) t.key(k);
    expect(t.row("Cargo.toml").getAttribute("aria-selected")).toBe("true");
    // Shift alone, as for a capital letter, keeps the prefix.
    for (const k of ["Home", "s", "Shift", "R"]) t.key(k);
    expect(t.row("src.rs").getAttribute("aria-selected")).toBe("true");
  });

  it("keeps the open file's preview tab on Enter", async () => {
    const t = mountTree(new FakeCore(dirsFromPaths(["a.ts", "b.ts"])), "a.ts");
    await settle();
    t.key("ArrowDown");
    t.key("ArrowDown");
    t.key("Enter");
    expect([t.opened, t.kept]).toEqual([["b.ts"], []]);
    t.key("ArrowUp");
    t.key("Enter");
    expect(t.kept).toEqual(["a.ts"]);
  });

  it("collapses all, filters to changed files and reveals the open file from the toolbar", async () => {
    let revealed = 0;
    const core = new FakeCore(dirsFromPaths(["a/b/c.ts", "a/d.ts"], { a: "modified", "a/d.ts": "modified" }));
    const target = document.body.appendChild(document.createElement("div"));
    const app = mount(FileTree, {
      target,
      props: { transport: core, project: "p1", openPath: "a/b/c.ts", onopen: () => {}, onreveal: () => revealed++ },
    });
    cleanup = () => unmount(app);
    await settle();
    const button = (name: string) =>
      [...target.querySelectorAll("button")].find((b) => b.getAttribute("aria-label") === name) as HTMLButtonElement;
    const paths = () => [...target.querySelectorAll<HTMLElement>("[role=treeitem]")].map((r) => r.dataset.path);
    // Icon buttons have short names and tooltips.
    expect(button("Reveal").title).toBe("Reveal the open file");
    expect(button("Changed only").title).toBe("Show changed files only");
    button("Reveal").click();
    await settle();
    // The sidebar is asked to show the tree first, also from the Git or Search view.
    expect(revealed).toBe(1);
    expect(paths()).toEqual(["a", "a/b", "a/b/c.ts", "a/d.ts"]);
    expect(target.querySelector("[aria-selected=true]")?.getAttribute("data-path")).toBe("a/b/c.ts");
    button("Changed only").click();
    await settle();
    expect(paths()).toEqual(["a", "a/d.ts"]);
    expect(button("Changed only").getAttribute("aria-pressed")).toBe("true");
    button("Collapse all").click();
    await settle();
    expect(paths()).toEqual(["a"]);
  });

  it("keeps the rows under the user's eyes in place when files appear above", async () => {
    const core = new FakeCore(new Map([["", flatDir(1000)]]));
    const t = mountTree(core);
    await settle();
    let top = 28 * 500;
    Object.defineProperty(t.tree(), "scrollTop", { get: () => top, set: (v: number) => (top = v), configurable: true });
    t.tree().dispatchEvent(new Event("scroll"));
    flushSync();
    core.dirs = new Map([["", [...flatDir(3, "a"), ...flatDir(1000)]]]);
    core.emit({ type: "fs.changed", data: { project: "p1", paths: ["a000000", "a000001", "a000002"] } });
    await settle();
    expect(top).toBe(28 * 503);
  });

  it("reveals and selects the open file without taking focus", async () => {
    const core = new FakeCore(dirsFromPaths(["a/b/c.ts", "z.md"]));
    const target = document.body.appendChild(document.createElement("div"));
    const app = mount(FileTree, {
      target,
      props: { transport: core, project: "p1", openPath: "a/b/c.ts", onopen: () => {} },
    });
    cleanup = () => unmount(app);
    await settle();
    expect(target.querySelector("[aria-selected=true]")?.getAttribute("data-path")).toBe("a/b/c.ts");
  });

  it("marks a folder that could not load when the core is disconnected", async () => {
    const core = new FakeCore(dirsFromPaths(["src/a.ts"]));
    const t = mountTree(core);
    await settle();
    let calls = 0;
    core.call = (() => {
      calls++;
      return calls > 20 ? new Promise(() => {}) : Promise.reject(new Error("connection to core closed"));
    }) as typeof core.call;
    t.key("ArrowDown");
    t.key("ArrowRight");
    await settle();
    expect(calls).toBe(1);
    const src = t.row("src");
    expect(src.getAttribute("aria-expanded")).toBe("true");
    expect(document.getElementById(src.getAttribute("aria-describedby") ?? "")?.textContent).toBe(
      "could not load, collapse and expand to retry",
    );
  });

  it("re-reads the tree after the connection comes back", async () => {
    const core = new FakeCore(dirsFromPaths(["a.ts"]));
    const t = mountTree(core);
    await settle();
    core.setStatus("closed");
    core.dirs = dirsFromPaths(["a.ts", "made-offline.ts"]);
    core.setStatus("open");
    await settle();
    expect(t.rows().map((r) => r.dataset.path)).toEqual(["a.ts", "made-offline.ts"]);
  });

  it("selects a regenerated file again when it comes back", async () => {
    const core = new FakeCore(dirsFromPaths(["gen/b.rs", "src/util.rs"]));
    const t = mountTree(core);
    await settle();
    t.row("gen").click();
    await settle();
    t.key("ArrowDown");
    expect(t.row("gen/b.rs").getAttribute("aria-selected")).toBe("true");
    core.dirs = dirsFromPaths(["src/util.rs"]);
    core.emit({ type: "fs.changed", data: { project: "p1", paths: ["gen"] } });
    await settle();
    core.dirs = dirsFromPaths(["gen/b.rs", "src/util.rs"]);
    core.emit({ type: "fs.changed", data: { project: "p1", paths: ["gen"] } });
    await settle();
    expect(t.row("gen/b.rs").getAttribute("aria-selected")).toBe("true");
  });

  it("goes to the next and previous changed file", async () => {
    const t = mountTree(
      new FakeCore(dirsFromPaths(["a.ts", "b.ts", "c.ts", "d.ts"], { "b.ts": "modified", "d.ts": "added" })),
    );
    await settle();
    const run = (id: string) =>
      listCommands()
        .find((c) => c.id === id)
        ?.run();
    await run("tree.next_change");
    flushSync();
    expect(t.row("b.ts").getAttribute("aria-selected")).toBe("true");
    await run("tree.next_change");
    flushSync();
    expect(t.row("d.ts").getAttribute("aria-selected")).toBe("true");
    await run("tree.previous_change");
    flushSync();
    expect(t.row("b.ts").getAttribute("aria-selected")).toBe("true");
  });

  it("shows why the tree could not load", async () => {
    const core = new FakeCore(new Map());
    const target = document.body.appendChild(document.createElement("div"));
    const app = mount(FileTree, { target, props: { transport: core, project: "p1", onopen: () => {} } });
    cleanup = () => unmount(app);
    await settle();
    expect(target.querySelector("[role=alert]")?.textContent).toBe("no such directory: ");
  });
});

describe("ProjectList", () => {
  it("lists projects and opens one", async () => {
    const core = new FakeCore(undefined, [
      { id: "p1", name: "ovra", root: "/src/ovra", last_opened_ms: null, trusted: false },
    ]);
    const opened: string[] = [];
    const target = document.body.appendChild(document.createElement("div"));
    const app = mount(ProjectList, {
      target,
      props: { transport: core, opener: new FolderOpener(core, (p) => opened.push(p.id)) },
    });
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
    const app = mount(ProjectList, {
      target,
      props: { transport: core, opener: new FolderOpener(core, (p) => opened.push(p.name)) },
    });
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

  it("explains a full file watch limit until it is dismissed or lifted", async () => {
    const core = new FakeCore(dirsFromPaths(["a.ts"]));
    mountTree(core);
    await settle();
    const notice = () => document.querySelector<HTMLElement>("[data-testid=watch-limit-notice]");
    expect(notice()).toBeNull();

    core.emit({ type: "fs.watch_limited", data: { project: "other", unwatched: 5 } });
    flushSync();
    expect(notice()).toBeNull();
    core.emit({ type: "fs.watch_limited", data: { project: "p1", unwatched: 3 } });
    flushSync();
    expect(notice()?.textContent).toContain("3 folders are not watched");
    expect(notice()?.textContent).toContain("sudo sysctl fs.inotify.max_user_watches=524288");

    notice()?.querySelector("button")?.click();
    flushSync();
    expect(notice()).toBeNull();
    // More folders without a watch: worth telling again.
    core.emit({ type: "fs.watch_limited", data: { project: "p1", unwatched: 2 } });
    flushSync();
    expect(notice()).toBeNull();
    core.emit({ type: "fs.watch_limited", data: { project: "p1", unwatched: 4 } });
    flushSync();
    expect(notice()).not.toBeNull();
    core.emit({ type: "fs.watch_limited", data: { project: "p1", unwatched: 0 } });
    flushSync();
    expect(notice()).toBeNull();
  });
});
