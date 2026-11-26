// Copies files and folders dropped from the system's file manager onto the
// tree into the project. The app keeps the dropped paths; the tree only
// names the drop's id and the folder under the pointer (`importDrop`). A
// name that exists already is asked about first: Replace (the old one goes
// to the Trash), Keep both, or Cancel.

import type { ProjectId } from "../lib/protocol";
import type { DropClash, Point, Transport, WindowDrop } from "../lib/transport";

type Host = {
  transport: Transport;
  project: ProjectId;
  /** The folder a drop at `at` goes into (`""` is the project folder), or null when it is not on the tree. */
  dirAt: (at: Point) => string | null;
  /** The copied entries, to select and reveal. */
  copied: (paths: string[]) => void;
  fail: (message: string) => void;
  tell: (message: string) => void;
};

export type ClashAsk = { drop: WindowDrop; dir: string; text: string };

const quoted = (name: string) => `“${name}”`;

/** "“a.png” already exists in src." for the names a drop shares with `dir`. */
export function clashQuestion(names: string[], dir: string): string {
  const where = dir === "" ? "the project folder" : dir;
  const [first] = names;
  if (names.length === 1) return `${quoted(first as string)} already exists in ${where}.`;
  const more = names.length - 1;
  return `${quoted(first as string)} and ${more} more ${more === 1 ? "item" : "items"} already exist in ${where}.`;
}

/** Says which links were left out of a copy, or null when none were. */
export function skippedNote(links: string[]): string | null {
  const [first] = links;
  if (first === undefined) return null;
  if (links.length === 1) return `Skipped ${quoted(first)}: links are not copied.`;
  return `Skipped ${links.length} links, such as ${quoted(first)}: links are not copied.`;
}

export class DropImporter {
  /** The folder files dragged in from outside would drop into. */
  over = $state<string | null>(null);
  /** The clash the person is asked about. */
  asking = $state<ClashAsk | null>(null);
  private busy = false;

  constructor(private readonly host: Host) {}

  /** Whether this window can import drops at all (not in headless mode). */
  get available(): boolean {
    return this.host.transport.importDrop !== undefined;
  }

  hover(at: Point | null): void {
    this.over = at ? this.host.dirAt(at) : null;
  }

  /** A drop onto the window: imports it when it landed on the tree. */
  async drop(drop: WindowDrop): Promise<void> {
    this.over = null;
    const dir = this.host.dirAt(drop);
    if (dir === null || this.busy) return;
    this.asking = null;
    await this.run(drop, dir, "ask");
  }

  /** The person's answer to the clash question; null cancels. */
  async answer(clash: Exclude<DropClash, "ask"> | null): Promise<void> {
    const ask = this.asking;
    this.asking = null;
    if (ask && clash) await this.run(ask.drop, ask.dir, clash);
  }

  private async run(drop: WindowDrop, dir: string, clash: DropClash): Promise<void> {
    const { transport, project } = this.host;
    if (!transport.importDrop) return;
    this.busy = true;
    try {
      const done = await transport.importDrop(drop.id, project, dir, clash);
      if (done.kind === "clashes") {
        this.asking = { drop, dir, text: clashQuestion(done.names, dir) };
        return;
      }
      if (done.paths.length > 0) this.host.copied(done.paths);
      const note = skippedNote(done.skipped_links);
      if (note) this.host.tell(note);
    } catch (err) {
      this.host.fail(err instanceof Error ? err.message : String(err));
    } finally {
      this.busy = false;
    }
  }
}
