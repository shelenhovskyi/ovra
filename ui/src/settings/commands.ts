// Settings' commands: open the view, and each pane on its own, so the
// palette finds "proxy" or "permissions" and goes straight there.

import { registerCommand } from "../keys/commands";
import { mainView } from "../shell/main-view.svelte";
import { PANES, type PaneId, settings } from "./panes.svelte";

/** Shows Settings, on `pane` or the one it showed last. */
export function openSettings(pane?: PaneId): void {
  if (pane) settings.pane = pane;
  mainView.show("settings");
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
      run: () => openSettings(),
    }),
    ...PANES.map((pane) =>
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
