// The project's file list for "Go to file", built from `fs.list_dir` and
// cached per project until the core reports a change. Deleted and ignored
// entries (and so `.git`, `node_modules`, `target`) are skipped.
//
// The walk lives in the UI until the core offers a file search; it is capped
// so a huge repository cannot stall the window.

import type { ProjectId } from "../lib/protocol";
import type { Transport } from "../lib/transport";

const MAX_FILES = 50_000;
const PARALLEL = 8;

async function walk(transport: Transport, project: ProjectId): Promise<string[]> {
  const files: string[] = [];
  let pending = [""];
  while (pending.length > 0 && files.length < MAX_FILES) {
    const batch = pending.slice(0, PARALLEL);
    pending = pending.slice(PARALLEL);
    const listings = await Promise.all(
      batch.map((path) => transport.call({ method: "fs.list_dir", params: { project, path } })),
    );
    for (const listing of listings) {
      for (const entry of listing.data.entries) {
        if (entry.ignored || entry.git === "deleted") continue;
        const path = listing.data.path ? `${listing.data.path}/${entry.name}` : entry.name;
        if (entry.kind === "dir") pending.push(path);
        else files.push(path);
      }
    }
  }
  return files.slice(0, MAX_FILES).sort();
}

const cache = new Map<ProjectId, Promise<string[]>>();
const watched = new WeakSet<Transport>();

/** All non-ignored files of `project`, sorted; reuses the last walk until the files change. */
export function projectFiles(transport: Transport, project: ProjectId): Promise<string[]> {
  if (!watched.has(transport)) {
    watched.add(transport);
    transport.onEvent((event) => {
      if (event.type === "fs.changed") cache.delete(event.data.project);
    });
  }
  let files = cache.get(project);
  if (!files) {
    files = walk(transport, project);
    files.catch(() => cache.delete(project));
    cache.set(project, files);
  }
  return files;
}
