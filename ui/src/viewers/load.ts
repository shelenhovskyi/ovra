// Fetches the open file for the viewer and decides when to fetch it again.

import type { CoreEvent, ProjectId } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";
import { type View, viewFor } from "./kind";

export type Loaded = { view: View } | { error: string };

export async function loadView(transport: Transport, project: ProjectId, path: string): Promise<Loaded> {
  try {
    const file = await transport.call({ method: "fs.read_file", params: { project, path } });
    return { view: viewFor(file.data.path, file.data.body) };
  } catch (err) {
    if (err instanceof CoreError && err.code === "not_found") return { error: `${path} no longer exists.` };
    return { error: err instanceof Error ? err.message : String(err) };
  }
}

/** Whether `event` means the file at `path` in `project` may have changed. */
export function touches(event: CoreEvent, project: ProjectId, path: string): boolean {
  if (event.type === "info.resync") return true;
  return event.type === "fs.changed" && event.data.project === project && event.data.paths.includes(path);
}
