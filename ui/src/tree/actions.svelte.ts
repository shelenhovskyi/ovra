// What the person does to files in the tree: new file or folder, rename,
// duplicate, trash, move, copy path, reveal. Holds the inline name edit and
// the trash question; every change goes to the core (`fs.*`), and the tree
// reloads the folders it touched instead of waiting for the watcher.

import { absolutePath } from "../editor/context-menu";
import type { EntryKind, ProjectId } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";
import { draftPath, joinPath, parentOf, type TreeModel } from "./model";
import { baseName, copyName, moveTarget, nameProblem } from "./ops";

const NOTICE_MS = 5000;

export type Edit = { kind: "new"; dir: string; entry: EntryKind } | { kind: "rename"; path: string; entry: EntryKind };

export type ActionsHost = {
  transport: Transport;
  project: ProjectId;
  model: TreeModel;
  /** The project folder on disk, for Copy Path. */
  root: () => string;
  selected: () => string | null;
  /** Selects a row and puts focus back on the tree. */
  select: (path: string | null) => void;
  /** Opens a file the person just made, in the editor. */
  open: (path: string) => void;
  moved?: (from: string, to: string) => void;
  trashed?: (paths: string[]) => void;
  copy?: (text: string) => Promise<void>;
};

export class FileActions {
  edit = $state<Edit | null>(null);
  /** Why the typed name cannot be used, shown under the input. */
  problem = $state<string | null>(null);
  /** The path waiting for a Move to Trash answer. */
  confirming = $state<string | null>(null);
  /** A failed action, shown briefly. */
  notice = $state<string | null>(null);
  private before: string | null = null;
  private busy = false;
  private timer: ReturnType<typeof setTimeout> | undefined;

  constructor(private readonly host: ActionsHost) {}

  /** Starts naming a new file or folder in `dir`. */
  async startNew(dir: string, entry: EntryKind): Promise<void> {
    this.cancel(false);
    this.before = this.host.selected();
    if (dir !== "") await this.host.model.expand(dir);
    this.edit = { kind: "new", dir, entry };
    this.host.model.setDraft({ dir, kind: entry });
    this.host.select(draftPath(dir));
  }

  startRename(path: string): void {
    this.cancel(false);
    const name = baseName(path);
    const entry = this.host.model.entries(parentOf(path))?.find((e) => e.name === name);
    if (!entry) return;
    this.before = path;
    this.edit = { kind: "rename", path, entry: entry.kind };
    this.host.select(path);
  }

  /** Checks a typed name; returns (and shows) the problem, or null. */
  check(name: string): string | null {
    const edit = this.edit;
    if (!edit) return null;
    const dir = edit.kind === "new" ? edit.dir : parentOf(edit.path);
    const siblings = this.host.model.entries(dir) ?? [];
    this.problem =
      edit.kind === "new"
        ? nameProblem(name, siblings, { nested: true })
        : nameProblem(name, siblings, { nested: false, current: baseName(edit.path) });
    return this.problem;
  }

  /** Enter (or leaving the input with `blur`) commits; a name that cannot be used cancels on blur. */
  async commit(name: string, blur = false): Promise<void> {
    const edit = this.edit;
    if (!edit || this.busy) return;
    if (edit.kind === "rename" && name === baseName(edit.path)) return this.cancel();
    if (this.check(name)) {
      if (blur) this.cancel();
      return;
    }
    this.busy = true;
    try {
      if (edit.kind === "new") await this.create(edit.dir, name, edit.entry);
      else await this.rename(edit.path, name);
    } catch (err) {
      this.cancel();
      this.fail(err);
    } finally {
      this.busy = false;
    }
  }

  /** Ends the name edit without a change. */
  cancel(refocus = true): void {
    if (!this.edit) return;
    const wasNew = this.edit.kind === "new";
    this.end();
    if (refocus || wasNew) this.host.select(this.before);
  }

  private end(): void {
    if (this.edit?.kind === "new") this.host.model.setDraft(null);
    this.edit = null;
    this.problem = null;
  }

  private async create(dir: string, name: string, entry: EntryKind): Promise<void> {
    const path = joinPath(dir, name);
    const { transport, project, model } = this.host;
    await transport.call({ method: "fs.create", params: { project, path, dir: entry === "dir" } });
    this.end();
    await model.reload(dir);
    await model.reveal(path);
    this.host.select(path);
    if (entry !== "dir") this.host.open(path);
  }

  private async rename(from: string, name: string): Promise<void> {
    const to = joinPath(parentOf(from), name);
    const { transport, project, model } = this.host;
    await transport.call({ method: "fs.rename", params: { project, from, to } });
    model.moved(from, to);
    this.end();
    await model.reload(parentOf(from));
    this.host.select(to);
    this.host.moved?.(from, to);
  }

  /** Copies `path` next to itself as `name copy.ext`, then starts renaming the copy. */
  async duplicate(path: string): Promise<void> {
    const { transport, project, model } = this.host;
    const dir = parentOf(path);
    const siblings = model.entries(dir) ?? [];
    const entry = siblings.find((e) => e.name === baseName(path));
    if (!entry) return;
    const to = joinPath(
      dir,
      copyName(
        entry.name,
        entry.kind === "dir",
        siblings.map((e) => e.name),
      ),
    );
    try {
      await transport.call({ method: "fs.copy", params: { project, from: path, to } });
      await model.reload(dir);
      this.startRename(to);
    } catch (err) {
      this.fail(err);
    }
  }

  /** Asks before moving `path` to the trash. */
  askTrash(path: string): void {
    this.cancel(false);
    this.confirming = path;
  }

  async confirmTrash(): Promise<void> {
    const path = this.confirming;
    this.confirming = null;
    if (path === null) return;
    const { transport, project, model } = this.host;
    try {
      await transport.call({ method: "fs.trash", params: { project, paths: [path] } });
      await model.reload(parentOf(path));
      this.host.trashed?.([path]);
    } catch (err) {
      this.fail(err);
    }
    this.host.select(this.host.selected());
  }

  cancelTrash(): void {
    const path = this.confirming;
    this.confirming = null;
    this.host.select(path);
  }

  /** Moves `from` into the folder `dir` (`""` is the project folder). */
  async move(from: string, dir: string): Promise<void> {
    const to = moveTarget(from, dir);
    if (to === null) return;
    const { transport, project, model } = this.host;
    try {
      await transport.call({ method: "fs.rename", params: { project, from, to } });
      model.moved(from, to);
      await model.reload(parentOf(from));
      await model.reload(dir);
      await model.reveal(to);
      this.host.select(to);
      this.host.moved?.(from, to);
    } catch (err) {
      this.fail(err);
    }
  }

  async copyPath(path: string, relative: boolean): Promise<void> {
    const text = relative ? path : absolutePath(this.host.root(), path);
    const copy = this.host.copy ?? ((t: string) => navigator.clipboard.writeText(t));
    try {
      await copy(text);
    } catch {
      this.fail(new Error("Could not copy"));
    }
  }

  async reveal(path: string): Promise<void> {
    const { transport, project } = this.host;
    try {
      await transport.call({ method: "open.reveal", params: { project, path } });
    } catch (err) {
      this.fail(err);
    }
  }

  private fail(err: unknown): void {
    this.notice = err instanceof CoreError ? err.error.message : err instanceof Error ? err.message : String(err);
    clearTimeout(this.timer);
    this.timer = setTimeout(() => (this.notice = null), NOTICE_MS);
  }
}
