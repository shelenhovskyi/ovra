// Whether this project's agents run in a sandbox here, as the Network and
// Servers panes say it: the same fact the session shows, from the newest
// session that reported how it runs. The next one most likely runs the
// same way.

import type { SessionInfo } from "../lib/protocol";

/** `none` when the newest session that said so runs without a sandbox; null while none said. */
export function projectSandbox(sessions: SessionInfo[]): "none" | "sandboxed" | null {
  const known = [...sessions].reverse().find((s) => s.sandbox_reported || s.sandbox);
  if (!known) return null;
  return known.sandbox ? "sandboxed" : "none";
}
