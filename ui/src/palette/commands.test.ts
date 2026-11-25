import { describe, expect, it } from "vitest";
import { listCommands, registerCommand } from "../keys/commands";
import type { Project } from "../lib/protocol";
import type { Transport } from "../lib/transport";
import { registerPaletteCommands } from "./commands";
import { type PaletteMode, palette } from "./mode.svelte";
import { source } from "./sources";

const project: Project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: false };
const run = (id: string) =>
  listCommands()
    .find((c) => c.id === id)
    ?.run();

describe("palette commands", () => {
  it("open the palette in the right mode and close the project", async () => {
    const workspace = { paletteOpen: false, project: null as Project | null, openPath: null as string | null };
    const unregister = registerPaletteCommands(workspace);
    expect(listCommands().map((c) => [c.id, c.keys])).toEqual([
      ["palette.close_project", undefined],
      ["palette.go_to_file", "Mod+P"],
      ["palette.shortcuts", undefined],
      ["palette.settings", "Mod+,"],
      ["palette.search", "Mod+K"],
      ["palette.show", "Mod+Shift+P"],
      ["palette.switch_project", undefined],
    ]);

    await run("palette.go_to_file");
    expect([workspace.paletteOpen, palette.mode]).toEqual([true, "projects"]);
    workspace.project = project;
    await run("palette.go_to_file");
    expect(palette.mode).toBe("files");
    palette.mode = "commands";
    await run("palette.search");
    expect(palette.mode).toBe("search");
    await run("palette.show");
    expect(palette.mode).toBe("commands");
    // Settings lists the commands that open one, Keyboard shortcuts those with keys.
    const off = registerCommand({ id: "t.rules", title: "Rules", category: "Settings", run: () => {} });
    await run("palette.settings");
    expect(palette.mode).toBe("settings");
    const labels = async (mode: PaletteMode) =>
      (await source(mode, {} as Transport, { ...workspace, mode: "view", openFile: () => {} }).entries()).map(
        (e) => e.label,
      );
    expect(await labels("settings")).toEqual(["Rules"]);
    expect(await labels("shortcuts")).toEqual(["Go to file", "Open settings", "Search or run", "Show all commands"]);
    off();

    // The start screen, as the tree shows it once the project is closed.
    const screen = document.createElement("section");
    screen.dataset.testid = "project-list";
    screen.innerHTML = "<ul><li><button>demo</button></li></ul><input />";
    document.body.append(screen);
    workspace.openPath = "a.md";
    await run("palette.close_project");
    expect([workspace.project, workspace.openPath]).toEqual([null, null]);
    expect(document.activeElement).toBe(screen.querySelector("button"));
    screen.remove();
    unregister();
    expect(listCommands()).toEqual([]);
  });
});
