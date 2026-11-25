import { describe, expect, it } from "vitest";
import { FileActions } from "./actions.svelte";
import { dirsFromPaths, FakeCore } from "./fake-core";
import { TreeModel } from "./model";

async function setup(paths: string[]) {
  const core = new FakeCore(dirsFromPaths(paths));
  const model = new TreeModel(
    async (path) => (await core.call({ method: "fs.list_dir", params: { project: "p1", path } })).data.entries,
    () => {},
  );
  await model.start();
  const log = {
    selected: null as string | null,
    opened: [] as string[],
    moved: [] as string[],
    trashed: [] as string[],
  };
  const copied: string[] = [];
  const actions = new FileActions({
    transport: core,
    project: "p1",
    model,
    root: () => "/work/app",
    selected: () => log.selected,
    select: (path) => {
      log.selected = path;
    },
    open: (path) => log.opened.push(path),
    moved: (from, to) => log.moved.push(`${from} -> ${to}`),
    trashed: (paths) => log.trashed.push(...paths),
    copy: async (text) => {
      copied.push(text);
    },
  });
  const paths_ = () => model.rows.map((r) => r.path);
  return { core, model, actions, log, copied, paths: paths_ };
}

describe("FileActions", () => {
  it("names a new file in a draft row, makes its parents and opens it", async () => {
    const t = await setup(["src/a.ts", "README.md"]);
    await t.actions.startNew("src", "file");
    expect(t.paths()).toEqual(["src", "src/", "src/a.ts", "README.md"]);
    expect(t.log.selected).toBe("src/");

    expect(t.actions.check("a.ts")).toBe("“a.ts” exists");
    await t.actions.commit("a.ts");
    expect(t.actions.edit).not.toBeNull();

    await t.actions.commit("lib/b.ts");
    expect(t.actions.edit).toBeNull();
    expect(t.paths()).toEqual(["src", "src/lib", "src/lib/b.ts", "src/a.ts", "README.md"]);
    expect(t.log).toMatchObject({ selected: "src/lib/b.ts", opened: ["src/lib/b.ts"] });
  });

  it("cancels a draft on Escape or on leaving it with a bad name, and selects what was selected", async () => {
    const t = await setup(["README.md"]);
    t.log.selected = "README.md";
    await t.actions.startNew("", "dir");
    t.actions.cancel();
    expect(t.paths()).toEqual(["README.md"]);
    expect(t.log.selected).toBe("README.md");

    await t.actions.startNew("", "dir");
    await t.actions.commit("", true);
    expect(t.actions.edit).toBeNull();
    expect(t.core.calls.some((c) => c.method === "fs.create")).toBe(false);
  });

  it("renames in place, and an unchanged name is no call", async () => {
    const t = await setup(["src/a.ts"]);
    await t.model.expand("src");
    t.actions.startRename("src/a.ts");
    await t.actions.commit("a.ts");
    expect(t.core.calls.some((c) => c.method === "fs.rename")).toBe(false);

    t.actions.startRename("src/a.ts");
    await t.actions.commit("b.ts");
    expect(t.paths()).toEqual(["src", "src/b.ts"]);
    expect(t.log.moved).toEqual(["src/a.ts -> src/b.ts"]);

    // A renamed folder stays open, and so do the open folders in it.
    const u = await setup(["src/lib/c.ts"]);
    await u.model.expand("src");
    await u.model.expand("src/lib");
    u.actions.startRename("src");
    await u.actions.commit("app");
    expect(u.model.expandedDirs()).toEqual(["app", "app/lib"]);
  });

  it("duplicates as a free copy name and starts renaming it", async () => {
    const t = await setup(["a.ts", "a copy.ts"]);
    await t.actions.duplicate("a.ts");
    expect(t.paths()).toEqual(["a copy 2.ts", "a copy.ts", "a.ts"]);
    expect(t.actions.edit).toEqual({ kind: "rename", path: "a copy 2.ts", entry: "file" });
  });

  it("trashes only after the answer, and shows a failure briefly", async () => {
    const t = await setup(["src/a.ts", "b.ts"]);
    t.actions.askTrash(["src"]);
    expect(t.actions.confirming).toEqual(["src"]);
    t.actions.cancelTrash();
    expect(t.paths()).toEqual(["src", "b.ts"]);

    t.actions.askTrash(["src"]);
    await t.actions.confirmTrash();
    expect(t.paths()).toEqual(["b.ts"]);
    expect(t.log.trashed).toEqual(["src"]);

    t.actions.askTrash(["gone.ts"]);
    await t.actions.confirmTrash();
    expect(t.actions.notice).toBe("gone.ts not found");
  });

  it("moves into a folder, refuses a folder into itself, and reports a clash", async () => {
    const t = await setup(["src/inner/x.ts", "lib/a.ts", "a.ts"]);
    await t.actions.move(["src"], "src/inner");
    expect(t.core.calls.some((c) => c.method === "fs.rename")).toBe(false);

    await t.actions.move(["a.ts"], "lib");
    expect(t.core.calls.filter((c) => c.method === "fs.rename")).toHaveLength(1);
    expect(t.actions.notice).toBe("lib/a.ts exists");

    await t.actions.move(["lib/a.ts"], "src");
    expect(t.log).toMatchObject({ selected: "src/a.ts", moved: ["lib/a.ts -> src/a.ts"] });
    expect(t.paths()).toContain("src/a.ts");
  });

  it("trashes several at once and moves several, skipping what is inside another or already there", async () => {
    const t = await setup(["src/a.ts", "lib/b.ts", "c.ts", "d.ts"]);
    t.actions.askTrash(["src/a.ts", "src", "c.ts"]);
    expect(t.actions.confirming).toEqual(["src", "c.ts"]);
    await t.actions.confirmTrash();
    expect(t.paths()).toEqual(["lib", "d.ts"]);
    expect(t.log.trashed).toEqual(["src", "c.ts"]);

    await t.model.expand("lib");
    await t.actions.move(["lib/b.ts", "d.ts", "lib"], "lib");
    expect(t.core.calls.filter((c) => c.method === "fs.rename")).toEqual([
      { method: "fs.rename", params: { project: "p1", from: "d.ts", to: "lib/d.ts" } },
    ]);
    expect(t.log.selected).toBe("lib/d.ts");
  });

  it("copies absolute and relative paths and reveals through the core", async () => {
    const t = await setup(["src/a.ts"]);
    await t.actions.copyPaths(["src/a.ts"], false);
    await t.actions.copyPaths(["src/a.ts", "b.ts"], true);
    await t.actions.reveal("");
    expect(t.copied).toEqual(["/work/app/src/a.ts", "src/a.ts\nb.ts"]);
    expect(t.core.revealed).toEqual([""]);
  });
});
