// Which list the palette shows when it opens. Kept here rather than in the
// workspace state: only the palette and its commands care.

export type PaletteMode = "commands" | "files" | "projects";

export const palette = $state<{ mode: PaletteMode }>({ mode: "commands" });
