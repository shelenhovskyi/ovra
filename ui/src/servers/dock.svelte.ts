// The Servers tab in the bottom dock, next to the terminals: whether it is
// shown, whether it shows every work dir or which one, and how many of its
// servers run (the status bar says so while the tab is hidden). Settings'
// Servers pane starts servers and shows them here.

import type { SessionId } from "../lib/protocol";

export const serversDock = $state<{
  shown: boolean;
  /** Every work dir of the project: the main checkout and each worker's worktree. */
  everywhere: boolean;
  /** The one work dir shown unless `everywhere`; null is the main checkout. */
  workDir: SessionId | null;
  running: number;
  started: number;
}>({
  shown: false,
  everywhere: true,
  workDir: null,
  running: 0,
  /** Bumped when the pane started a server, so the tab lists it at once. */
  started: 0,
});
