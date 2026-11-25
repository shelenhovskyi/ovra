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

describe("rowMenu", () => {
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
