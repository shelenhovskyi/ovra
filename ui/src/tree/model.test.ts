import { describe, expect, it } from "vitest";
import { type Dirs, dirsFromPaths, FakeCore, flatDir } from "./fake-core";
import { sortEntries, TreeModel, typeaheadMatch, visibleRange } from "./model";

function setup(paths: string[] | Dirs, git = {}) {
  const core = new FakeCore(Array.isArray(paths) ? dirsFromPaths(paths, git) : paths);
  let changes = 0;
  const model = new TreeModel(
    async (path) => (await core.call({ method: "fs.list_dir", params: { project: "p1", path } })).data.entries,
    () => changes++,
  );
  const names = () => model.rows.map((r) => `${"  ".repeat(r.depth)}${r.name}${r.expanded ? "/" : ""}`);
  return { core, model, names, changes: () => changes };
}

describe("TreeModel", () => {
  it("loads the root, then directories lazily on expand", async () => {
    const { core, model, names } = setup(["src/a.ts", "src/lib/b.ts", "README.md"]);
    await model.start();
    expect(names()).toEqual(["src", "README.md"]);
    expect(core.listedPaths()).toEqual([""]);

    await model.expand("src");
    expect(names()).toEqual(["src/", "  lib", "  a.ts", "README.md"]);
    await model.expand("src/lib");
    expect(names()).toEqual(["src/", "  lib/", "    b.ts", "  a.ts", "README.md"]);
    expect(core.listedPaths()).toEqual(["", "src", "src/lib"]);
  });

  it("keeps loaded children when collapsing and re-expanding", async () => {
    const { core, model, names } = setup(["src/lib/b.ts"]);
    await model.start();
    await model.expand("src");
    await model.expand("src/lib");
    model.collapse("src");
    expect(names()).toEqual(["src"]);
    await model.toggle("src");
    expect(names()).toEqual(["src/", "  lib/", "    b.ts"]);
    expect(core.listedPaths()).toEqual(["", "src", "src/lib"]);
  });

  it("restores expanded directories and drops the ones that are gone", async () => {
    const { model, names } = setup(["src/lib/b.ts", "docs/a.md"]);
    await model.start(["src/lib", "src", "old"]);
    expect(names()).toEqual(["docs", "src/", "  lib/", "    b.ts"]);
    expect(model.expandedDirs()).toEqual(["src", "src/lib"]);
  });

  it("marks an expanding directory as loading until its listing arrives", async () => {
    const { model } = setup(["src/a.ts"]);
    await model.start();
    const done = model.expand("src");
    expect(model.rows[0]).toMatchObject({ path: "src", expanded: true, loading: true });
    await done;
    expect(model.rows[0]).toMatchObject({ loading: false });
  });

  it("carries git status and ignored flags into rows", async () => {
    const { model } = setup(["a.ts", ".env"], { "a.ts": "modified" });
    await model.start();
    expect(model.rows.map((r) => [r.name, r.git, r.ignored])).toEqual([
      [".env", null, true],
      ["a.ts", "modified", false],
    ]);
  });

  it("dims everything inside an ignored directory", async () => {
    const dirs = dirsFromPaths(["dist/app.js", "src/a.ts"]);
    // The core marks only the top ignored entry; its contents inherit the flag.
    const dist = dirs.get("")?.find((e) => e.name === "dist");
    if (dist) dist.ignored = true;
    const { model } = setup(dirs);
    await model.start();
    await model.expand("dist");
    await model.expand("src");
    expect(model.rows.map((r) => [r.path, r.ignored])).toEqual([
      ["dist", true],
      ["dist/app.js", true],
      ["src", false],
      ["src/a.ts", false],
    ]);
  });

  it("reloads only the directories an fs.changed event touches", async () => {
    const { core, model, names } = setup(["src/a.ts", "docs/x.md"]);
    await model.start();
    await model.expand("src");
    core.dirs = dirsFromPaths(["src/a.ts", "src/new.ts", "docs/x.md"]);
    core.calls = [];
    await model.invalidatePaths(["src/new.ts", "docs/x.md"]);
    // `docs` is not loaded, so only `src` (parent of new.ts) reloads.
    expect(core.listedPaths()).toEqual(["src"]);
    expect(names()).toEqual(["docs", "src/", "  a.ts", "  new.ts"]);
  });

  it("refreshes git status of every loaded directory on fs.git_changed", async () => {
    const { core, model } = setup(["src/a.ts"]);
    await model.start();
    await model.expand("src");
    core.dirs = dirsFromPaths(["src/a.ts"], { src: "modified", "src/a.ts": "modified" });
    await model.invalidateAll();
    expect(model.rows.map((r) => r.git)).toEqual(["modified", "modified"]);
  });

  it("forgets the listings of a deleted directory and lists it afresh when it returns", async () => {
    const { core, model, names } = setup(["src/lib/b.ts", "a.ts"]);
    await model.start();
    await model.expand("src");
    await model.expand("src/lib");
    core.dirs = dirsFromPaths(["a.ts"]);
    await model.invalidatePaths(["src"]);
    expect(names()).toEqual(["a.ts"]);
    // Re-created later: listed afresh, and open as it was (UX round 3, ORCH-3).
    core.calls = [];
    core.dirs = dirsFromPaths(["src/lib/b.ts", "src/lib/c.ts", "a.ts"]);
    await model.invalidatePaths(["src"]);
    expect(names()).toEqual(["src/", "  lib/", "    b.ts", "    c.ts", "a.ts"]);
    expect(core.listedPaths().sort()).toEqual(["", "src", "src/lib"]);
  });

  it("reports load errors", async () => {
    const model = new TreeModel(
      () => Promise.reject(new Error("unavailable: file access is not implemented yet")),
      () => {},
    );
    await model.start();
    expect(model.error).toBe("unavailable: file access is not implemented yet");
  });

  it("leaves deleted tracked files out of the full tree, but not out of the changed view", async () => {
    const git = {
      "old.md": "deleted",
      gone: "deleted",
      "gone/x.ts": "deleted",
      src: "deleted",
      trashed: "deleted",
    } as const;
    const { model, names } = setup(["gone/x.ts", "trashed/y.ts", "src/a.ts", "old.md", "new.md"], git);
    await model.start();
    // An unloaded folder with that status may hold files that exist: it stays.
    expect(names()).toEqual(["gone", "src", "trashed", "new.md"]);
    await model.expand("gone"); // holds only deleted files
    await model.expand("src");
    model.removed(["trashed"]); // the person trashed it
    model.collapse("src");
    expect(names()).toEqual(["src", "new.md"]);
    await model.setFilter("changed");
    expect(names()).toEqual(["gone/", "  x.ts", "src/", "trashed/", "old.md"]);
  });

  // 20k rows already make quadratic work obvious; tests/perf times the 100k-file repo.
  it("flattens 20k files in one directory in few updates", async () => {
    const big = flatDir(20_000);
    const { model, changes } = setup(
      new Map([
        ["", [{ name: "big", kind: "dir", ignored: false, git: null }]],
        ["big", big],
      ]),
    );
    await model.start();
    await model.expand("big");
    expect(model.rows).toHaveLength(20_001);
    expect(model.rows[20_000]?.path).toBe("big/f019999");
    expect(changes()).toBeLessThan(10);
  });
});

describe("TreeModel restore", () => {
  it("loads restored folders only once their row is on screen", async () => {
    const paths = Array.from({ length: 50 }, (_, i) => `pkg${i}/src/a.ts`);
    const { core, model } = setup(paths);
    await model.setView(new Set());
    const saved = paths.flatMap((p) => [p.split("/")[0] as string, p.split("/").slice(0, 2).join("/")]);
    await model.start(saved);
    expect(core.listedPaths()).toEqual([""]);
    await model.setView(new Set(["pkg0", "pkg1"])); // the first rows come on screen
    expect(core.listedPaths().sort()).toEqual(["", "pkg0", "pkg1"]);
    await model.setView(new Set(["pkg0", "pkg0/src"]));
    expect(core.listedPaths()).toContain("pkg0/src");
    expect(core.listedPaths()).not.toContain("pkg1/src");
    expect(model.expandedDirs()).toHaveLength(100); // the saved shape is kept
  });
});

describe("TreeModel with the core disconnected", () => {
  it("does not retry a failed folder in a loop, and retries after expand or resync", async () => {
    const { core, model } = setup(["src/a.ts"]);
    await model.start();
    let down = true;
    const call = core.call.bind(core);
    core.call = ((request) =>
      down ? Promise.reject(new Error("connection to core closed")) : call(request)) as typeof core.call;
    let listed = 0;
    const counting = core.call;
    core.call = ((request) => {
      listed++;
      // Park a runaway retry loop instead of spinning, so a regression fails instead of hanging the run.
      return listed > 20 ? new Promise(() => {}) : counting(request);
    }) as typeof core.call;

    await model.expand("src");
    await model.setView(null);
    await model.invalidatePaths([]);
    expect(listed).toBe(1);
    expect(model.rows[0]).toMatchObject({ path: "src", expanded: true, loading: false, failed: true });
    expect(model.error).toBe("connection to core closed");

    down = false;
    await model.invalidateAll(); // what a resync after reconnecting does
    expect(model.rows.map((r) => r.path)).toEqual(["src", "src/a.ts"]);
    expect(model.rows[0]?.failed).toBe(false);
  });

  it("retries a failed folder when it is expanded again", async () => {
    const { core, model } = setup(["src/a.ts"]);
    await model.start();
    const call = core.call.bind(core);
    let down = true;
    core.call = ((request) => (down ? Promise.reject(new Error("closed")) : call(request))) as typeof core.call;
    await model.expand("src");
    down = false;
    model.collapse("src");
    await model.expand("src");
    expect(model.rows.map((r) => r.path)).toEqual(["src", "src/a.ts"]);
  });
});

describe("TreeModel restore and regenerate", () => {
  it("reveals a file inside restored folders that have not loaded yet", async () => {
    const { model, names } = setup(["a/b/c.ts", "z.md"]);
    await model.setView(new Set()); // nothing on screen yet, so restored folders wait
    await model.start(["a", "a/b"]);
    expect(names()).toEqual(["a/", "z.md"]);
    await model.reveal("a/b/c.ts");
    expect(names()).toEqual(["a/", "  b/", "    c.ts", "z.md"]);
  });

  it("keeps a folder open when it is deleted and written again", async () => {
    const { core, model, names } = setup(["gen/a.rs", "src/main.rs"]);
    await model.start(["gen"]);
    core.dirs = dirsFromPaths(["src/main.rs"]);
    await model.invalidatePaths(["gen"]);
    expect(names()).toEqual(["src"]);
    core.dirs = dirsFromPaths(["gen/a.rs", "gen/b.rs", "src/main.rs"]);
    await model.invalidatePaths(["gen"]);
    expect(names()).toEqual(["gen/", "  a.rs", "  b.rs", "src"]);
  });
});

describe("TreeModel refresh", () => {
  const tree = ["a/x.ts", "b/y.ts", "c/z.ts"];

  it("on fs.git_changed reloads only listings on screen, the rest when shown again", async () => {
    const { core, model } = setup(tree);
    await model.start(["a", "b", "c"]);
    model.collapse("c");
    await model.setView(new Set(["a", "a/x.ts"])); // rows of "" and of a are on screen
    core.calls = [];
    await model.invalidateAll();
    expect(core.listedPaths().sort()).toEqual(["", "a"]);
    core.calls = [];
    await model.setView(new Set(["b/y.ts"])); // scrolled to b's children
    expect(core.listedPaths()).toEqual(["b"]);
    core.calls = [];
    await model.expand("c"); // collapsed while stale: shown from cache at once...
    expect(core.listedPaths()).toEqual([]);
    expect(model.rows.map((r) => r.path)).toContain("c/z.ts");
    await model.setView(new Set(["c/z.ts"])); // ...and reloaded once its rows are on screen
    expect(core.listedPaths()).toEqual(["c"]);
  });

  it("does not show the loading hint when reloading a listing it already has", async () => {
    const { model } = setup(tree);
    await model.start(["a"]);
    const seen: boolean[] = [];
    const done = model.invalidateAll();
    seen.push(model.rows.some((r) => r.loading));
    await done;
    expect(seen).toEqual([false]);
  });

  it("reloads everything loaded under a folder the watcher reports as one path", async () => {
    const { core, model } = setup(["pkg/src/deep/a.ts"]);
    await model.start(["pkg", "pkg/src", "pkg/src/deep"]);
    core.calls = [];
    await model.invalidatePaths(["pkg"]);
    expect(core.listedPaths().sort()).toEqual(["", "pkg", "pkg/src", "pkg/src/deep"]);
  });

  it("re-lists shown ignored folders on request, since the core does not watch them", async () => {
    const dirs = dirsFromPaths(["dist/a.js", "src/b.ts"]);
    const dist = dirs.get("")?.find((e) => e.name === "dist");
    if (dist) dist.ignored = true;
    const { core, model } = setup(dirs);
    await model.start(["dist", "src"]);
    core.calls = [];
    await model.invalidateIgnored();
    expect(core.listedPaths()).toEqual(["dist"]);
  });
});

describe("TreeModel navigation", () => {
  it("sorts naturally, folders first, and hides .git", () => {
    const entry = (name: string, kind: "file" | "dir" = "file") => ({ name, kind, ignored: false, git: null });
    const sorted = sortEntries([
      entry("module-10"),
      entry("module-2"),
      entry("b", "dir"),
      entry(".git", "dir"),
      entry("README.md"),
      entry("a file"),
    ]);
    expect(sorted.map((e) => e.name)).toEqual(["b", "a file", "module-2", "module-10", "README.md"]);
  });

  it("numbers rows among their siblings", async () => {
    const { model } = setup(["src/a.ts", "src/b.ts", "z.md"]);
    await model.start(["src"]);
    expect(model.rows.map((r) => [r.path, r.posinset, r.setsize])).toEqual([
      ["src", 1, 2],
      ["src/a.ts", 1, 2],
      ["src/b.ts", 2, 2],
      ["z.md", 2, 2],
    ]);
  });

  it("collapses everything and reveals a deep file", async () => {
    const { model, names } = setup(["a/b/c/d.ts", "z.md"]);
    await model.start(["a"]);
    model.collapseAll();
    expect(names()).toEqual(["a", "z.md"]);
    await model.reveal("a/b/c/d.ts");
    expect(names()).toEqual(["a/", "  b/", "    c/", "      d.ts", "z.md"]);
  });

  it("shows changed files only, opening folders with changes, and restores the tree after", async () => {
    const git = {
      src: "modified",
      "src/lib": "modified",
      "src/lib/a.ts": "modified",
      "docs/new.md": "untracked",
      docs: "untracked",
    } as const;
    const { model, names } = setup(["src/lib/a.ts", "src/lib/b.ts", "src/c.ts", "docs/new.md", "README.md"], git);
    await model.start(["src"]);
    await model.setFilter("changed");
    // docs is wholly untracked, so it stays closed.
    expect(names()).toEqual(["docs", "src/", "  lib/", "    a.ts"]);
    expect(model.expandedDirs()).toEqual(["src"]);
    await model.setFilter("all");
    expect(names()).toEqual(["docs", "src/", "  lib", "  c.ts", "README.md"]);
  });

  it("finds the next row by typed prefix, wrapping around", async () => {
    const { model } = setup(["alpha.ts", "beta.ts", "bravo.ts"]);
    await model.start();
    expect(typeaheadMatch(model.rows, 0, "b")).toBe(1);
    expect(typeaheadMatch(model.rows, 2, "BR")).toBe(2);
    expect(typeaheadMatch(model.rows, 2, "a")).toBe(0);
    expect(typeaheadMatch(model.rows, 0, "x")).toBe(-1);
  });
});

describe("visibleRange", () => {
  it("covers the viewport plus overscan, clamped to the list", () => {
    expect(visibleRange(0, 220, 22, 1000, 5)).toEqual({ start: 0, end: 15 });
    expect(visibleRange(2200, 220, 22, 1000, 5)).toEqual({ start: 95, end: 115 });
    expect(visibleRange(21_900, 220, 22, 1000, 5)).toEqual({ start: 990, end: 1000 });
    expect(visibleRange(0, 220, 22, 0, 5)).toEqual({ start: 0, end: 0 });
  });
});
