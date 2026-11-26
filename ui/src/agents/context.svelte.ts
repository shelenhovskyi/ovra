// What the person adds to a task or reply with `@` (docs/specs/task-context.md).
// Other areas offer what applies now (`registerContextSource`: the editor's
// selection and problems, the terminal's output) and hand one item over with
// `askAgent`; the Agents panel keeps the chips per box, has the core check
// each one as it is added and sends them with the message.

import { SvelteMap } from "svelte/reactivity";
import type { ContextItem, ContextWarning, ContextWhat, ProjectId } from "../lib/protocol";
import type { Transport } from "../lib/transport";
import type { IconName } from "../shell/icons";

/** Something the @ menu offers. */
export type Suggestion = { what: ContextWhat; label: string; detail: string };

/** An added item as its chip shows it. */
export type Chip = {
  key: string;
  what: ContextWhat;
  label: string;
  /** The full name, for the tooltip. */
  title: string;
  icon: IconName;
  /** Why it waits for Send anyway; null when it goes as it is. */
  warning: ContextWarning | null;
  confirmed: boolean;
};

/** The box a chip belongs to: a project's new-task box (`new:<project>`), or a session's reply box. */
export type Target = string;

const sources = new Map<string, () => Suggestion[]>();

/** Offers what `source` returns in the @ menu (in registration order) until the returned function runs. */
export function registerContextSource(id: string, source: () => Suggestion[]): () => void {
  sources.set(id, source);
  return () => {
    if (sources.get(id) === source) sources.delete(id);
  };
}

/** What every source offers now. */
export function contextSuggestions(): Suggestion[] {
  return [...sources.values()].flatMap((source) => source());
}

class Asks {
  /** Items handed over with Ask agent, not yet taken by the Agents panel. */
  waiting = $state<ContextWhat[]>([]);
}

export const agentAsks = new Asks();

/** Adds `what` to the Agents panel's open box, shows the panel and focuses the field. */
export function askAgent(what: ContextWhat): void {
  agentAsks.waiting = [...agentAsks.waiting, what];
}

function base(path: string): string {
  const trimmed = path.replace(/\/+$/, "");
  return trimmed.slice(trimmed.lastIndexOf("/") + 1) || trimmed;
}

/** The first lines of `text`, for a tooltip that shows what goes. */
function preview(text: string): string {
  const lines = text.split("\n");
  const shown = lines.slice(0, 6).map((line) => (line.length > 80 ? `${line.slice(0, 80)}…` : line));
  return lines.length > 6 ? [...shown, "…"].join("\n") : shown.join("\n");
}

/** A chip's short name, full name (with the text it sends) and icon. */
export function describeItem(what: ContextWhat): Pick<Chip, "label" | "title" | "icon"> {
  switch (what.kind) {
    case "file":
      return { label: base(what.path), title: what.path, icon: "file" };
    case "folder":
      return { label: `${base(what.path)}/`, title: `${what.path.replace(/\/+$/, "")}/`, icon: "files" };
    case "selection": {
      const lines = what.from_line === what.to_line ? `${what.from_line}` : `${what.from_line}-${what.to_line}`;
      return {
        label: `${base(what.path)}:${lines}`,
        title: `${what.path}, lines ${lines}\n${preview(what.text)}`,
        icon: "selection",
      };
    }
    case "problem":
      return {
        label: `${base(what.path)}:${what.line}`,
        title: `${what.path}:${what.line} ${what.message}`,
        icon: "problem",
      };
    case "terminal":
      return {
        label: what.title || "Terminal",
        title: `Output of ${what.title}\n${preview(what.text)}`,
        icon: "terminal",
      };
    case "page":
      return { label: "Screenshot", title: "What the page shows when you send", icon: "image" };
  }
}

/** Two items name the same thing: adding it again does nothing. */
function same(a: ContextWhat, b: ContextWhat): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

/** The warning on a chip, in words. */
export const WARNINGS: Record<ContextWarning, string> = {
  protected: "Agent settings file",
  secret_file: "May hold secrets",
  secret_text: "May hold a secret",
};

let made = 0;

export class ContextDrafts {
  readonly chips = new SvelteMap<Target, Chip[]>();

  constructor(private readonly transport: Transport) {}

  of(target: Target): Chip[] {
    return this.chips.get(target) ?? [];
  }

  /** Adds `what` once the core checked it; rejects with the core's reason (a path outside the project). */
  async add(project: ProjectId, target: Target, what: ContextWhat): Promise<void> {
    if (this.of(target).some((chip) => same(chip.what, what))) return;
    const session = target.startsWith("new:") ? undefined : target;
    const answer = await this.transport.call({
      method: "context.check",
      params: { project, items: [what], ...(session ? { session } : {}) },
    });
    if (this.of(target).some((chip) => same(chip.what, what))) return;
    made += 1;
    const chip: Chip = {
      key: `c${made}`,
      what,
      ...describeItem(what),
      warning: answer.data.notes[0] ?? null,
      confirmed: false,
    };
    this.chips.set(target, [...this.of(target), chip]);
  }

  /** The person chose Send anyway on its warning. */
  confirm(target: Target, key: string): void {
    this.set(
      target,
      this.of(target).map((chip) => (chip.key === key ? { ...chip, confirmed: true } : chip)),
    );
  }

  remove(target: Target, key: string): void {
    this.set(
      target,
      this.of(target).filter((chip) => chip.key !== key),
    );
  }

  /** Chips that wait for Send anyway. */
  unconfirmed(target: Target): Chip[] {
    return this.of(target).filter((chip) => chip.warning !== null && !chip.confirmed);
  }

  /** What goes with the message: every chip, the confirmed ones marked. */
  items(target: Target): ContextItem[] {
    return this.of(target).map((chip) => ({ ...chip.what, confirmed: chip.confirmed }));
  }

  clear(target: Target): void {
    this.chips.delete(target);
  }

  private set(target: Target, chips: Chip[]): void {
    if (chips.length > 0) this.chips.set(target, chips);
    else this.chips.delete(target);
  }
}

const instances = new WeakMap<Transport, ContextDrafts>();

export function contextDrafts(transport: Transport): ContextDrafts {
  let drafts = instances.get(transport);
  if (!drafts) {
    drafts = new ContextDrafts(transport);
    instances.set(transport, drafts);
  }
  return drafts;
}

/** The `@query` (or full-width `＠query`) the caret is in: where it starts and what follows `@`; null outside one. */
export function atQuery(text: string, caret: number): { start: number; query: string } | null {
  const before = text.slice(0, caret);
  const found = /(^|\s)[@\uFF20]([^\s@\uFF20]*)$/.exec(before);
  if (!found) return null;
  const query = found[2] ?? "";
  return { start: caret - query.length - 1, query };
}
