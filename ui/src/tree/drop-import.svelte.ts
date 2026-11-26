// Copies files and folders dropped from the system's file manager onto the
// tree into the project. The app keeps the dropped paths; the tree only
// names the drop's id and the folder under the pointer (`importDrop`). A
// name that looks like a secret store is asked about first (Copy anyway or
// Cancel: a copy in the project is there for every agent; only the dropped
// names and their folders are checked, not what a dropped folder holds), and
// so is a name
// that exists already: Replace (the old one goes to the Trash), Keep both,
// or Cancel.

import type { ProjectId } from "../lib/protocol";
import type { DropClash, Point, Transport, WindowDrop } from "../lib/transport";

type Host = {
  transport: Transport;
  project: ProjectId;
  /** The folder a drop at `at` goes into (`""` is the project folder), or null when it is not on the tree. */
  dirAt: (at: Point) => string | null;
  /** The names among the dropped paths that look like secrets (`secretLooking`). */
  secretNames: (paths: string[]) => Promise<string[]>;
  /** The copied entries, to select and reveal. */
  copied: (paths: string[]) => void;
  fail: (message: string) => void;
  tell: (message: string) => void;
};

/** A question about a drop: a secret-looking name, or names that exist already. */
export type DropAsk = { kind: "secret" | "clash"; drop: WindowDrop; dir: string; text: string };

/** The person's answer: "copy" to a secret question, a clash answer to a clash, null cancels. */
export type DropAnswer = "copy" | Exclude<DropClash, "ask"> | null;

const quoted = (name: string) => `“${name}”`;

/** "“a.png” already exists in src." for the names a drop shares with `dir`. */
export function clashQuestion(names: string[], dir: string): string {
  const where = dir === "" ? "the project folder" : dir;
  const [first] = names;
  if (names.length === 1) return `${quoted(first as string)} already exists in ${where}.`;
  const more = names.length - 1;
  return `${quoted(first as string)} and ${more} more ${more === 1 ? "item" : "items"} already exist in ${where}.`;
}

/** "“.env” may hold secrets. …" for the secret-looking names of a drop. */
export function secretQuestion(names: string[]): string {
  const [first] = names;
  const more = names.length - 1;
  const who =
    more === 0
      ? quoted(first as string)
      : `${quoted(first as string)} and ${more} more ${more === 1 ? "item" : "items"}`;
  return `${who} may hold secrets. A copy in the project can be read by every agent.`;
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
  /** The question the person is asked about a drop. */
  asking = $state<DropAsk | null>(null);
  private busy = false;

  constructor(private readonly host: Host) {}

  /** Whether this window can import drops at all (not in headless mode). */
  get available(): boolean {
    return this.host.transport.importDrop !== undefined;
  }

  hover(at: Point | null): void {
    this.over = at ? this.host.dirAt(at) : null;
  }

  /** A drop onto the window: imports it when it landed on the tree, after Copy anyway for secret-looking names. */
  async drop(drop: WindowDrop): Promise<void> {
    this.over = null;
    const dir = this.host.dirAt(drop);
    if (dir === null || this.busy) return;
    this.asking = null;
    this.busy = true;
    let risky: string[];
    try {
      risky = await this.host.secretNames(drop.paths);
    } catch (err) {
      this.host.fail(err instanceof Error ? err.message : String(err));
      return;
    } finally {
      this.busy = false;
    }
    if (risky.length > 0) {
      this.asking = { kind: "secret", drop, dir, text: secretQuestion(risky) };
      return;
    }
    await this.run(drop, dir, "ask");
  }

  /** The person's answer to the question asked. */
  async answer(choice: DropAnswer): Promise<void> {
    const ask = this.asking;
    this.asking = null;
    if (!ask || choice === null) return;
    if (ask.kind === "secret") {
      if (choice === "copy") await this.run(ask.drop, ask.dir, "ask");
    } else if (choice !== "copy") {
      await this.run(ask.drop, ask.dir, choice);
    }
  }

  private async run(drop: WindowDrop, dir: string, clash: DropClash): Promise<void> {
    const { transport, project } = this.host;
    if (!transport.importDrop) return;
    this.busy = true;
    try {
      const done = await transport.importDrop(drop.id, project, dir, clash);
      if (done.kind === "clashes") {
        this.asking = { kind: "clash", drop, dir, text: clashQuestion(done.names, dir) };
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
