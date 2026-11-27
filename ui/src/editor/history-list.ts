// "Local history": the kept versions of the open file in the palette;
// choosing one puts its text in the editor as an edit the person can undo
// and has to save, so nothing on disk changes by picking.

import { formatShortcut, isApple } from "../keys/shortcut";
import { showList } from "../palette/mode.svelte";
import type { Workspace } from "../shell/workspace.svelte";
import { type LocalHistory, localHistory, versionLabel } from "./local-history";
import { tabs } from "./open-files.svelte";

type Target = Pick<Workspace, "openPath" | "mode" | "paletteOpen">;

/** Lists the open file's versions; with none yet, says when they are kept. */
export async function showLocalHistory(
  workspace: Target,
  edit: () => Promise<void>,
  history: LocalHistory = localHistory,
): Promise<void> {
  const path = workspace.openPath;
  const buffers = tabs.current?.buffers;
  if (!path || !buffers) return;
  const versions = await history.versions(buffers.project, path);
  if (workspace.openPath !== path) return;
  const now = Date.now();
  const current = buffers.get(path)?.text;
  if (versions.length === 0) {
    tabs.notice = `No earlier versions of ${path} yet. One is kept each time it opens, is saved or changes on disk.`;
    return;
  }
  const undo = formatShortcut("Mod+Z", isApple());
  const entries = versions.map((version, index) => {
    const label = versionLabel(version, now);
    const lines = version.text.split("\n").length;
    return {
      id: `history-${index}`,
      label,
      detail: version.text === current ? "same as now" : `${lines} ${lines === 1 ? "line" : "lines"}`,
      run: async () => {
        await edit();
        const buffer = tabs.current?.buffers.get(path);
        const doc = buffer?.doc;
        if (!buffer || !doc || workspace.openPath !== path) return;
        buffer.edit({ changes: { from: 0, to: doc.length, insert: version.text }, userEvent: "input.restore" });
        tabs.notice = `Put back the text ${label.charAt(0).toLowerCase()}${label.slice(1)}. Save keeps it; ${undo} brings back what was there.`;
      },
    };
  });
  showList(
    {
      name: "Local history",
      placeholder: `Pick a version of ${path}`,
      empty: "No version matches.",
      limit: entries.length,
      entries: () => entries,
      idle: (all) => all,
    },
    workspace,
  );
}
