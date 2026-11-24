// The project's file list for "Go to file", from `fs.list_files` (every
// non-ignored file, sorted, a page at a time), cached per project until the
// core reports a change.

import type { ProjectId } from "../lib/protocol";
import type { Transport } from "../lib/transport";

async function load(transport: Transport, project: ProjectId): Promise<string[]> {
  for (;;) {
    const first = (await transport.call({ method: "fs.list_files", params: { project } })).data;
    const paths = [...first.paths];
    let consistent = true;
    while (paths.length < first.total && consistent) {
      const page = (await transport.call({ method: "fs.list_files", params: { project, offset: paths.length } })).data;
      consistent = page.version === first.version && page.paths.length > 0;
      paths.push(...page.paths);
    }
    // The files changed between pages: start over, as the protocol asks.
    if (consistent) return paths;
  }
}

const cache = new Map<ProjectId, Promise<string[]>>();
const watched = new WeakSet<Transport>();

/** All non-ignored files of `project`, sorted; reuses the last list until the files change. */
export function projectFiles(transport: Transport, project: ProjectId): Promise<string[]> {
  if (!watched.has(transport)) {
    watched.add(transport);
    transport.onEvent((event) => {
      if (event.type === "fs.changed" || event.type === "fs.files_changed") cache.delete(event.data.project);
    });
  }
  let files = cache.get(project);
  if (!files) {
    files = load(transport, project);
    files.catch(() => cache.delete(project));
    cache.set(project, files);
  }
  return files;
}
