// Which list the palette shows when it opens. Kept here rather than in the
// workspace state: only the palette and its commands care.

/**
 * `folder` asks for the path of a folder to open, keeping the project open meanwhile.
 * `settings` and `shortcuts` list the commands that open a setting, or that have a shortcut.
 */
export type PaletteMode = "commands" | "files" | "projects" | "folder" | "settings" | "shortcuts";

export const palette = $state<{ mode: PaletteMode }>({ mode: "commands" });
