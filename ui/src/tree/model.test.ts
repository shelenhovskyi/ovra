import { describe, expect, it } from "vitest";
import { type Dirs, dirsFromPaths, FakeCore, flatDir } from "./fake-core";
import { TreeModel, visibleRange } from "./model";

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
    await model.refreshPaths(["src/new.ts", "docs/x.md"]);
    // `docs` is not loaded, so only `src` (parent of new.ts) reloads.
    expect(core.listedPaths()).toEqual(["src"]);
    expect(names()).toEqual(["docs", "src/", "  a.ts", "  new.ts"]);
  });

  it("refreshes git status of every loaded directory on fs.git_changed", async () => {
    const { core, model } = setup(["src/a.ts"]);
    await model.start();
    await model.expand("src");
    core.dirs = dirsFromPaths(["src/a.ts"], { src: "modified", "src/a.ts": "modified" });
    await model.refreshAll();
    expect(model.rows.map((r) => r.git)).toEqual(["modified", "modified"]);
  });

  it("forgets a deleted directory and everything under it", async () => {
    const { core, model, names } = setup(["src/lib/b.ts", "a.ts"]);
    await model.start();
    await model.expand("src");
    await model.expand("src/lib");
    core.dirs = dirsFromPaths(["a.ts"]);
    await model.refreshPaths(["src"]);
    expect(names()).toEqual(["a.ts"]);
    // Re-created later: it comes back collapsed and is listed afresh.
    core.dirs = dirsFromPaths(["src/lib/b.ts", "a.ts"]);
    await model.refreshPaths(["src"]);
    expect(names()).toEqual(["src", "a.ts"]);
    expect(model.isExpanded("src/lib")).toBe(false);
  });

  it("reports load errors", async () => {
    const model = new TreeModel(
      () => Promise.reject(new Error("unavailable: file access is not implemented yet")),
      () => {},
    );
    await model.start();
    expect(model.error).toBe("unavailable: file access is not implemented yet");
  });

  it("flattens 100k files in one directory quickly", async () => {
    const big = flatDir(100_000);
    const { model, changes } = setup(
      new Map([
        ["", [{ name: "big", kind: "dir", ignored: false, git: null }]],
        ["big", big],
      ]),
    );
    await model.start();
    await model.expand("big");
    expect(model.rows).toHaveLength(100_001);
    expect(model.rows[100_000]?.path).toBe("big/f099999");
    expect(changes()).toBeLessThan(10);
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
