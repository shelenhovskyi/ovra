import { describe, expect, it } from "vitest";
import type { MenuEntry, MenuItem } from "../menu/context-menu.svelte";
import { handleFindInFolder } from "../search/find-in";
import { handleOpenTerminalIn } from "../terminal/open-in";
import type { FileActions } from "./actions.svelte";
import { rowMenu } from "./menu";

const actions = { clip: null } as unknown as FileActions;

function item(entries: MenuEntry[], label: string): MenuItem | undefined {
  return entries.find((e): e is MenuItem => typeof e === "object" && e?.label === label);
}

const layout = (entries: MenuEntry[]) =>
  entries.filter(Boolean).map((e) => (e === "separator" ? "-" : (e as MenuItem).label));

describe("rowMenu", () => {
  it("keeps a folder's menu to 3 groups, with Paste only after a cut or copy", () => {
    const clipped = { clip: { paths: ["x"], cut: false } } as unknown as FileActions;
    const folder = rowMenu(clipped, { path: "src", kind: "dir" });
    expect(layout(folder)).toEqual([
      "New file…",
      "New folder…",
      "Find in folder…",
      expect.any(String), // Reveal in the OS file manager, named per OS
      "Open in terminal",
      "-",
      "Cut",
      "Copy",
      "Paste",
      "Copy path",
      "Duplicate",
      "Rename…",
      "-",
      "Move to Trash…",
    ]);
    expect(layout(rowMenu(actions, { path: "a.ts", kind: "file" }))).not.toContain("Paste");
  });

  it("offers only Copy path on a deleted file, which is gone from disk", () => {
    expect(layout(rowMenu(actions, { path: "a.ts", kind: "file", git: "deleted" }))).toEqual(["Copy path"]);
  });

  it("copies the relative path from Copy path's Alt variant", () => {
    const copied: [string[], boolean][] = [];
    const spy = { clip: null, copyPaths: (p: string[], rel: boolean) => void copied.push([p, rel]) };
    const copy = item(rowMenu(spy as unknown as FileActions, { path: "a.ts", kind: "file" }), "Copy path");
    copy?.run();
    copy?.alt?.run();
    expect(copy?.alt?.label).toBe("Copy relative path");
    expect(copied).toEqual([
      [["a.ts"], false],
      [["a.ts"], true],
    ]);
  });

  it("opens a terminal in a folder or a file's folder, and finds in folders only", () => {
    const terminals: string[] = [];
    const searches: string[] = [];
    const stop = [
      handleOpenTerminalIn((cwd) => void terminals.push(cwd)),
      handleFindInFolder((include) => void searches.push(include)),
    ];
    const folder = rowMenu(actions, { path: "src/ui", kind: "dir" });
    const file = rowMenu(actions, { path: "src/ui/app.ts", kind: "file" });
    const top = rowMenu(actions, { path: "a.ts", kind: "file" });
    for (const entries of [folder, file, top]) item(entries, "Open in terminal")?.run();
    item(folder, "Find in folder…")?.run();
    for (const off of stop) off();

    expect(terminals).toEqual(["src/ui", "src/ui", ""]);
    expect(searches).toEqual(["src/ui/**"]);
    expect(item(file, "Find in folder…")).toBeUndefined();
  });
});
