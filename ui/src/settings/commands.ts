// Settings' commands: open the window, and each pane on its own, so the
// palette finds "proxy" or "permissions" and goes straight there.

import { registerCommand } from "../keys/commands";
import { PANES, type PaneId, settings } from "./panes.svelte";
import { settingsWindow } from "./window.svelte";

/** Shows Settings, on `pane` or the one it showed last. */
export function openSettings(pane?: PaneId): void {
  if (pane) settings.pane = pane;
  settingsWindow.show();
}

/** Registers the commands; returns a function that unregisters them. */
export function registerSettingsCommands(): () => void {
  const off = [
    registerCommand({
      id: "settings.open",
      title: "Open settings",
      keys: "Mod+,",
      category: "Settings",
      keywords: ["preferences", "options"],
      // The key that opened Settings closes it, as the gear and Reviews' key do.
      run: () => (settingsWindow.open ? settingsWindow.close() : openSettings()),
    }),
    // An optional pane is reached from its switch, not from the palette.
    ...PANES.filter((pane) => !pane.optional).map((pane) =>
      registerCommand({
        id: `settings.${pane.id}`,
        title: pane.title,
        category: "Settings",
        keywords: pane.keywords,
        run: () => openSettings(pane.id),
      }),
    ),
  ];
  return () => {
    for (const unregister of off) unregister();
  };
}
