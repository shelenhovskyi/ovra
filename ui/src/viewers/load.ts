// Fetches the open file for the viewer and decides when to fetch it again.

import type { CoreEvent, ProjectId } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";
import { type View, viewFor } from "./kind";

export type Loaded = { view: View; size: number } | { error: string };

export async function loadView(transport: Transport, project: ProjectId, path: string): Promise<Loaded> {
  try {
    const file = await transport.call({ method: "fs.read_file", params: { project, path } });
    return { view: viewFor(file.data.path, file.data.body), size: file.data.size };
  } catch (err) {
    if (err instanceof CoreError && err.code === "not_found") return { error: `${path} no longer exists.` };
    if (err instanceof CoreError) return { error: `Could not read ${path}: ${err.error.message}` };
    // Anything else comes from the transport: the core is not reachable.
    return { error: `Not connected to Ovra's core, so ${path} cannot be read right now.` };
  }
}

/** Whether a project file exists, from its parent's listing (cheaper than reading it). */
export async function exists(transport: Transport, project: ProjectId, path: string): Promise<boolean> {
  const slash = path.lastIndexOf("/");
  const dir = slash < 0 ? "" : path.slice(0, slash);
  try {
    const listing = await transport.call({ method: "fs.list_dir", params: { project, path: dir } });
    const name = path.slice(slash + 1);
    return listing.data.entries.some((entry) => entry.name === name && entry.git !== "deleted");
  } catch {
    return false;
  }
}

/** "6.0 MB", "200 KB", "12 bytes". */
export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} ${bytes === 1 ? "byte" : "bytes"}`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/** Whether `event` means the file at `path` in `project` may have changed. */
export function touches(event: CoreEvent, project: ProjectId, path: string): boolean {
  if (event.type === "info.resync") return true;
  return event.type === "fs.changed" && event.data.project === project && event.data.paths.includes(path);
}
