// Which list the palette shows when it opens. Kept here rather than in the
// workspace state: only the palette and its commands care.

/**
 * `search` is "Search or run": files (projects without one open) and commands.
 * `folder` asks for the path of a folder to open, keeping the project open meanwhile.
 * `shortcuts` lists the commands that have a shortcut.
 */
export type PaletteMode = "search" | "commands" | "files" | "projects" | "folder" | "shortcuts";

export const palette = $state<{ mode: PaletteMode }>({ mode: "commands" });
