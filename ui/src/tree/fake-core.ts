// Test-only fake of the core for the tree: serves `fs.list_dir` from an
// in-memory listing per directory and `projects.*` from a list, and can push
// events. Mirrors the core's ordering: dirs first, then by name.

import type {
  CoreEvent,
  DirEntry,
  GitStatus,
  Project,
  ProjectId,
  ProjectState,
  Request,
  ResponseFor,
} from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";
import { joinPath } from "./model";

export type Dirs = Map<string, DirEntry[]>;

/** Builds listings from file paths; directories are implied by the paths. */
export function dirsFromPaths(paths: string[], git: Record<string, GitStatus> = {}): Dirs {
  const dirs: Dirs = new Map([["", []]]);
  for (const path of paths) {
    const parts = path.split("/");
    let dir = "";
    parts.forEach((name, i) => {
      const full = joinPath(dir, name);
      const kind = i === parts.length - 1 ? "file" : "dir";
      const list = dirs.get(dir) ?? [];
      dirs.set(dir, list);
      if (kind === "file" || !dirs.has(full)) {
        list.push({ name, kind, ignored: name.startsWith("."), git: git[full] ?? null });
        if (kind === "dir") dirs.set(full, []);
      }
      dir = full;
    });
  }
  for (const list of dirs.values()) list.sort((a, b) => sortKey(a).localeCompare(sortKey(b)));
  return dirs;
}

/** One flat directory of `count` files named `prefix000000`, `prefix000001`, ..., already in core order. */
export function flatDir(count: number, prefix = "f"): DirEntry[] {
  return Array.from({ length: count }, (_, i) => ({
    name: `${prefix}${String(i).padStart(6, "0")}`,
    kind: "file" as const,
    ignored: false,
    git: null,
  }));
}

function sortKey(e: DirEntry): string {
  return `${e.kind === "dir" ? 0 : 1}${e.name}`;
}

export class FakeCore implements Transport {
  calls: Request[] = [];
  states = new Map<string, ProjectState>();
  private listeners = new Set<(event: CoreEvent) => void>();

  constructor(
    public dirs: Dirs = new Map([["", []]]),
    public projects: Project[] = [],
  ) {}

  async call<R extends Request>(request: R): Promise<ResponseFor<R>> {
    this.calls.push(request);
    return this.answer(request) as ResponseFor<R>;
  }

  onEvent(listener: (event: CoreEvent) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  emit(event: CoreEvent): void {
    for (const listener of this.listeners) listener(event);
  }

  listedPaths(): string[] {
    return this.calls.flatMap((c) => (c.method === "fs.list_dir" ? [c.params.path] : []));
  }

  private answer(request: Request): unknown {
    switch (request.method) {
      case "fs.list_dir": {
        const entries = this.dirs.get(request.params.path);
        if (!entries) throw new CoreError({ code: "not_found", message: `no such directory: ${request.params.path}` });
        return { type: "fs.dir", data: { path: request.params.path, entries: [...entries] } };
      }
      case "projects.list":
        return { type: "projects.list", data: this.projects };
      case "projects.open": {
        const project = this.projects.find((p) => p.id === request.params.id);
        if (!project) throw new CoreError({ code: "not_found", message: "no such project" });
        return { type: "projects.project", data: project };
      }
      case "projects.add": {
        const root = request.params.root;
        const project = {
          id: `p${this.projects.length + 1}`,
          name: root.split("/").pop() ?? root,
          root,
          last_opened_ms: null,
        };
        this.projects.push(project);
        return { type: "projects.project", data: project };
      }
      case "projects.get_state":
        return { type: "projects.state", data: this.state(request.params.id) };
      case "projects.set_state":
        // Like the core: only the fields present are written.
        this.states.set(request.params.id, { ...this.state(request.params.id), ...request.params.state });
        return { type: "ok" };
      default:
        throw new CoreError({ code: "unavailable", message: `${request.method} is not faked` });
    }
  }

  private state(id: ProjectId): ProjectState {
    return this.states.get(id) ?? { open_files: [], active_file: null, expanded_dirs: [] };
  }
}
