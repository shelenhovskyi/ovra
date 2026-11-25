// An in-memory core for editor tests: files with mtimes (`fs.read_file`,
// `fs.write_file` with conflicts) and per-project state with partial updates.

import type { CoreEvent, FileBody, ProjectState, Request } from "../lib/protocol";
import { type ConnectionStatus, CoreError, type Transport } from "../lib/transport";

export class FakeCore implements Transport {
  files = new Map<string, { body: FileBody; mtime: number }>();
  writes: { path: string; text: string; expected: number | null }[] = [];
  state: ProjectState = { open_files: [], active_file: null, expanded_dirs: [] };
  sentStates: unknown[] = [];
  private clock = 100;
  private listeners = new Set<(event: CoreEvent) => void>();
  private statusListeners = new Set<(status: ConnectionStatus) => void>();
  private status: ConnectionStatus = "open";

  setStatus(status: ConnectionStatus): void {
    this.status = status;
    for (const listener of this.statusListeners) listener(status);
  }

  onStatus(listener: (status: ConnectionStatus) => void): () => void {
    this.statusListeners.add(listener);
    listener(this.status);
    return () => this.statusListeners.delete(listener);
  }

  /** Writes a file "on disk", as another program would, without an event. */
  put(path: string, text: string): void {
    this.files.set(path, { body: { kind: "text", text }, mtime: ++this.clock });
  }

  emit(event: CoreEvent): void {
    for (const listener of this.listeners) listener(event);
  }

  changed(...paths: string[]): void {
    this.emit({ type: "fs.changed", data: { project: "p1", paths } });
  }

  onEvent(listener: (event: CoreEvent) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  // biome-ignore lint/suspicious/noExplicitAny: test double for a generic method
  async call(request: Request): Promise<any> {
    switch (request.method) {
      case "fs.read_file": {
        const file = this.files.get(request.params.path);
        if (!file) throw new CoreError({ code: "not_found", message: `${request.params.path} not found` });
        return { type: "fs.file", data: { path: request.params.path, size: 0, mtime_ms: file.mtime, body: file.body } };
      }
      case "fs.write_file": {
        const { path, text, expected_mtime_ms: expected } = request.params;
        this.writes.push({ path, text, expected });
        const file = this.files.get(path);
        if (expected !== null && !file) {
          throw new CoreError({ code: "conflict", message: `${path} was deleted on disk` });
        }
        if (expected !== null && file && file.mtime !== expected) {
          throw new CoreError({ code: "conflict", message: `${path} changed on disk` });
        }
        this.put(path, text);
        return { type: "fs.written", data: { mtime_ms: this.clock } };
      }
      case "projects.get_state":
        return { type: "projects.state", data: this.state };
      case "projects.set_state":
        this.sentStates.push(request.params.state);
        this.state = { ...this.state, ...request.params.state };
        // As the core does: a second group without tabs is closed.
        if (this.state.second_group?.open_files.length === 0) delete this.state.second_group;
        return { type: "ok" };
      default:
        throw new Error(`unexpected ${request.method}`);
    }
  }
}

/** Storage in memory, for drafts. */
export class MemoryStorage implements Storage {
  private items = new Map<string, string>();
  get length(): number {
    return this.items.size;
  }
  clear(): void {
    this.items.clear();
  }
  getItem(key: string): string | null {
    return this.items.get(key) ?? null;
  }
  key(index: number): string | null {
    return [...this.items.keys()][index] ?? null;
  }
  removeItem(key: string): void {
    this.items.delete(key);
  }
  setItem(key: string, value: string): void {
    this.items.set(key, value);
  }
}
