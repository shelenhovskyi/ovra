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
import { type ConnectionStatus, CoreError, type Transport } from "../lib/transport";
import { joinPath, parentOf } from "./model";

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

  private statusListeners = new Set<(status: ConnectionStatus) => void>();

  onStatus(listener: (status: ConnectionStatus) => void): () => void {
    this.statusListeners.add(listener);
    listener("open");
    return () => this.statusListeners.delete(listener);
  }

  /** Simulates the connection dropping and coming back. */
  setStatus(status: ConnectionStatus): void {
    for (const listener of this.statusListeners) listener(status);
  }

  emit(event: CoreEvent): void {
    for (const listener of this.listeners) listener(event);
  }

  listedPaths(): string[] {
    return this.calls.flatMap((c) => (c.method === "fs.list_dir" ? [c.params.path] : []));
  }

  /** Paths `open.reveal` was asked to show. */
  revealed: string[] = [];

  private entry(path: string): DirEntry | undefined {
    const name = path.slice(path.lastIndexOf("/") + 1);
    return this.dirs.get(parentOf(path))?.find((e) => e.name === name);
  }

  /** Adds `entry` at `path`, making missing parent folders; `conflict` if something is there. */
  private put(path: string, entry: DirEntry): void {
    if (this.entry(path)) throw new CoreError({ code: "conflict", message: `${path} exists` });
    const parent = parentOf(path);
    if (!this.dirs.has(parent))
      this.put(parent, { name: parent.slice(parent.lastIndexOf("/") + 1), kind: "dir", ignored: false, git: null });
    const list = this.dirs.get(parent) as DirEntry[];
    list.push({ ...entry, name: path.slice(path.lastIndexOf("/") + 1) });
    list.sort((a, b) => sortKey(a).localeCompare(sortKey(b)));
    if (entry.kind === "dir" && !this.dirs.has(path)) this.dirs.set(path, []);
  }

  /** Copies (or with `keep: false` moves) `from` and everything under it to `to`. */
  private carry(from: string, to: string, keep: boolean): void {
    const entry = this.entry(from);
    if (!entry) throw new CoreError({ code: "not_found", message: `${from} not found` });
    this.put(to, entry);
    for (const [dir, list] of [...this.dirs]) {
      if (dir === from || dir.startsWith(`${from}/`)) this.dirs.set(to + dir.slice(from.length), [...list]);
    }
    if (!keep) this.remove(from);
  }

  private remove(path: string): void {
    const list = this.dirs.get(parentOf(path)) ?? [];
    this.dirs.set(
      parentOf(path),
      list.filter((e) => joinPath(parentOf(path), e.name) !== path),
    );
    for (const dir of [...this.dirs.keys()]) if (dir === path || dir.startsWith(`${path}/`)) this.dirs.delete(dir);
  }

  private answer(request: Request): unknown {
    switch (request.method) {
      case "fs.create":
        this.put(request.params.path, {
          name: "",
          kind: request.params.dir ? "dir" : "file",
          ignored: false,
          git: null,
        });
        return { type: "ok" };
      case "fs.rename":
      case "fs.copy":
        this.carry(request.params.from, request.params.to, request.method === "fs.copy");
        return { type: "ok" };
      case "fs.trash":
        for (const path of request.params.paths) {
          if (!this.entry(path)) throw new CoreError({ code: "not_found", message: `${path} not found` });
          this.remove(path);
        }
        return { type: "ok" };
      case "open.reveal":
        this.revealed.push(request.params.path);
        return { type: "ok" };
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
          trusted: false,
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
