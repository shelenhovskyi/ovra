import { describe, expect, it } from "vitest";
import { listCommands } from "../keys/commands";
import type { Project } from "../lib/protocol";
import { registerPaletteCommands } from "./commands";
import { palette } from "./mode.svelte";

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
    expect(palette.mode).toBe("files");
    await run("palette.show");
    expect(palette.mode).toBe("commands");

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
