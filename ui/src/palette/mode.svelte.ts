// Which list the palette shows when it opens. Kept here rather than in the
// workspace state: only the palette and its commands care.

/** `folder` asks for the path of a folder to open, keeping the project open meanwhile. */
export type PaletteMode = "commands" | "files" | "projects" | "folder";

export const palette = $state<{ mode: PaletteMode }>({ mode: "commands" });
