// Opens a folder as a project, however the person chose it: typed on the start
// screen, picked in the system's folder dialog, or dropped onto the window.
// Every path goes through `projects.add`, so the core validates them alike.

import type { Project } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";

/** A Unix path, a Windows drive path, or a UNC path (`\\server\share`). */
const ABSOLUTE = /^(\/|[A-Za-z]:[\\/]|\\\\)/;

/** A message for people: no protocol code, and a hint where there is one. */
export function folderError(err: unknown, path?: string): string {
  if (!(err instanceof CoreError)) return err instanceof Error ? err.message : String(err);
  switch (err.code) {
    case "not_found":
      return path ? `Folder not found: ${path}` : err.error.message;
    case "bad_request":
      return path && /not a (folder|directory)/i.test(err.error.message)
        ? "That is not a folder. Choose the folder itself, not a file in it."
        : err.error.message;
    case "unavailable":
      return "The project store is not available right now.";
    default:
      return err.error.message;
  }
}

export class FolderOpener {
  error = $state<string | null>(null);
  busy = $state(false);

  constructor(
    private readonly transport: Transport,
    private readonly onopen: (project: Project) => void,
  ) {}

  /** Whether the system's folder dialog is there to pick from. */
  get canPick(): boolean {
    return this.transport.pickFolder !== undefined;
  }

  /** Adds the folder at `path` and opens it; returns whether it opened. */
  async open(path: string): Promise<boolean> {
    if (this.busy) return false;
    this.error = null;
    if (!ABSOLUTE.test(path)) {
      this.error = "Use the full path to a folder, for example /home/you/my-app.";
      return false;
    }
    this.busy = true;
    try {
      const project = (await this.transport.call({ method: "projects.add", params: { root: path } })).data;
      this.onopen((await this.transport.call({ method: "projects.open", params: { id: project.id } })).data);
      return true;
    } catch (err) {
      this.error = folderError(err, path);
      return false;
    } finally {
      this.busy = false;
    }
  }

  /** Opens a project from the list of known ones. */
  async openKnown(project: Project): Promise<void> {
    if (this.busy) return;
    this.error = null;
    this.busy = true;
    try {
      this.onopen((await this.transport.call({ method: "projects.open", params: { id: project.id } })).data);
    } catch (err) {
      this.error = folderError(err, project.root);
    } finally {
      this.busy = false;
    }
  }

  /** Opens the folder the person picks in the system's dialog; nothing when they cancel. */
  async pick(): Promise<void> {
    if (!this.transport.pickFolder || this.busy) return;
    this.error = null;
    let path: string | null;
    // Busy while the dialog is up, so a drop meanwhile cannot open another folder.
    this.busy = true;
    try {
      path = await this.transport.pickFolder();
    } catch (err) {
      this.error = folderError(err);
      return;
    } finally {
      this.busy = false;
    }
    if (path) await this.open(path);
  }

  /** Opens a folder dropped onto the window. */
  async drop(paths: string[]): Promise<void> {
    const [path] = paths;
    if (path === undefined) return;
    if (paths.length > 1) {
      this.error = "Drop one folder at a time.";
      return;
    }
    await this.open(path);
  }
}
