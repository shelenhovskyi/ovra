import { describe, expect, it, vi } from "vitest";
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
  const unsaved: string[] = [];
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
    unsaved: (paths) => unsaved.filter((f) => paths.some((p) => f === p || f.startsWith(`${p}/`))),
    copy: async (text) => {
      copied.push(text);
    },
  });
  const paths_ = () => model.rows.map((r) => r.path);
  return { core, model, actions, log, copied, unsaved, paths: paths_ };
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

  it("copies and pastes, naming a clash as a copy, and puts the full paths on the system clipboard", async () => {
    const t = await setup(["src/a.ts", "lib/a.ts", "b.ts"]);
    await t.actions.copy(["src/a.ts", "b.ts", "src"]);
    expect(t.copied).toEqual(["/work/app/b.ts\n/work/app/src"]);
    await t.actions.paste("lib");
    expect(t.paths()).toEqual(["lib", "lib/src", "lib/a.ts", "lib/b.ts", "src", "b.ts"]);
    // A copy stays on the clipboard; pasting next to the originals makes copies.
    await t.actions.paste("");
    expect(t.paths().filter((p) => !p.startsWith("lib/"))).toEqual(["lib", "src", "src copy", "b copy.ts", "b.ts"]);
    expect(t.log.selected).toBe("src copy");

    await t.actions.paste("src");
    expect(t.actions.notice).toBe("Cannot paste a folder into itself");
  });

  it("cuts and pastes as a move once, and Escape forgets the cut", async () => {
    const t = await setup(["src/a.ts", "b.ts"]);
    t.actions.cut(["b.ts"]);
    expect(t.actions.dropCut()).toBe(true);
    expect(t.actions.clip).toBeNull();

    t.actions.cut(["b.ts"]);
    await t.actions.paste("");
    expect(t.actions.clip).not.toBeNull(); // nothing moved: still cut
    await t.actions.paste("src");
    expect(t.log.moved).toEqual(["b.ts -> src/b.ts"]);
    expect(t.actions.clip).toBeNull();
    expect(t.copied).toEqual([]);
  });

  it("undoes and redoes the person's own creates, renames, moves and copies", async () => {
    const t = await setup(["src/a.ts", "b.ts"]);
    await t.actions.startNew("", "file");
    await t.actions.commit("new.ts");
    t.actions.startRename("b.ts");
    await t.actions.commit("c.ts");
    await t.actions.copy(["c.ts"]);
    await t.actions.paste("src");
    await t.actions.move(["new.ts"], "src");
    expect(t.paths()).toEqual(["src", "src/a.ts", "src/c.ts", "src/new.ts", "c.ts"]);

    await t.actions.undo(); // the move
    await t.actions.undo(); // the pasted copy goes to the trash
    expect(t.paths()).toEqual(["src", "src/a.ts", "c.ts", "new.ts"]);
    expect(t.log.trashed).toEqual(["src/c.ts"]);
    await t.actions.undo(); // the rename
    await t.actions.undo(); // the new file goes to the trash
    expect(t.paths()).toEqual(["src", "src/a.ts", "b.ts"]);
    expect(t.log.moved.slice(-2)).toEqual(["src/new.ts -> new.ts", "c.ts -> b.ts"]);
    await t.actions.undo();
    expect(t.actions.info).toBe("Nothing to undo");

    await t.actions.redo();
    await t.actions.redo();
    expect(t.paths()).toEqual(["src", "src/a.ts", "c.ts", "new.ts"]);
    expect(t.core.calls.filter((c) => c.method === "fs.trash")).toHaveLength(2);
  });

  it("refuses to undo a move back onto a taken path, and says where trashed files went", async () => {
    const t = await setup(["a.ts"]);
    t.actions.startRename("a.ts");
    await t.actions.commit("b.ts");
    await t.core.call({ method: "fs.create", params: { project: "p1", path: "a.ts", dir: false } });
    await t.actions.undo();
    expect(t.actions.notice).toBe("Could not undo: a.ts exists");
    expect(t.paths()).toContain("b.ts");

    t.actions.askTrash(["b.ts"]);
    await t.actions.confirmTrash();
    expect(t.actions.info).toBe("Moved to Trash. Restore it from the Trash.");
  });

  it("stops undo at a trash instead of reaching past it, and says what each step did", async () => {
    const t = await setup(["src/lib.rs", "changes.patch"]);
    await t.model.expand("src");
    await t.actions.duplicate("src/lib.rs");
    t.actions.cancel();
    t.actions.askTrash(["changes.patch"]);
    await t.actions.confirmTrash();
    await t.actions.undo();
    await t.actions.undo();
    expect(t.actions.info).toBe("Can't undo Move to Trash. Restore it from the Trash.");
    expect(t.paths()).toContain("src/lib copy.rs");
    expect(t.core.calls.filter((c) => c.method === "fs.trash")).toHaveLength(1);

    t.actions.startRename("src/lib.rs");
    await t.actions.commit("a.rs");
    await t.actions.undo();
    expect(t.actions.info).toBe("Undid rename of lib.rs");
    await t.actions.redo();
    expect(t.actions.info).toBe("Redid rename of lib.rs");
    await t.actions.move(["src/a.rs", "src/lib copy.rs"], "");
    await t.actions.undo();
    expect(t.actions.info).toBe("Undid move of 2 items");
  });

  it("says what a trash takes along, and clears its note after 5 s or on the next action", async () => {
    vi.useFakeTimers();
    try {
      const t = await setup(["ui/a.ts", "ui/b.ts", "c.ts"]);
      t.actions.askTrash(["ui"]);
      expect(t.actions.question).toBe("Move “ui” to Trash?"); // not loaded: no count
      await t.model.expand("ui");
      t.unsaved.push("ui/b.ts");
      t.actions.askTrash(["ui"]);
      expect(t.actions.question).toBe("Move “ui” and the 2 items in it to Trash?");
      expect(t.actions.warning).toBe("“b.ts” has unsaved changes.");
      await t.actions.confirmTrash();
      expect(t.actions.info).toBe("Moved to Trash. Restore it from the Trash.");
      vi.advanceTimersByTime(5000);
      expect(t.actions.info).toBeNull();

      t.actions.askTrash(["c.ts"]);
      expect(t.actions.warning).toBeNull();
      await t.actions.confirmTrash();
      t.actions.startRename("c.ts");
      expect(t.actions.info).toBeNull();
    } finally {
      vi.useRealTimers();
    }
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
