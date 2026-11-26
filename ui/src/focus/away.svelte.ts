// "Where you left off" (docs/specs/focus-mode.md): after a break, one line
// at the top of Focus says what ended meanwhile and what waits now. A break
// is ten minutes or more without the person: the window was in the
// background, or nothing was typed or clicked in it. Pure apart from the
// line itself, so the phone companion can word its return the same way.

import type { SessionId } from "../lib/protocol";
import type { FocusRow } from "./model";

/** How long without the person counts as a break. */
export const BREAK_MS = 10 * 60_000;
/** At most this many ended sessions are named; the rest are counted. */
const NAMED = 2;
const NAME_CHARS = 60;

/** What Focus showed at one moment: the sessions that had ended and how many things waited. */
export type Moment = { done: FocusRow[]; waiting: number };

export class Away {
  /** The line to show, until the person dismisses it or the next break replaces it. */
  line = $state<string | null>(null);
  private last: { at: number; moment: Moment } | null = null;

  /** The person left (the window went to the background): remember what was on screen. */
  left(now: number, moment: Moment): void {
    this.last = { at: now, moment };
  }

  /** The person is here (typed, clicked, came back to the window). */
  here(now: number, moment: Moment): void {
    if (this.last && now - this.last.at >= BREAK_MS) {
      const line = awayLine(this.last.moment, moment);
      if (line) this.line = line;
    }
    this.last = { at: now, moment };
  }

  dismiss(): void {
    this.line = null;
  }
}

/**
 * "While you were away, Fix sync finished and 2 things wait for you.", or
 * null when nothing ended and nothing new waits.
 */
export function awayLine(before: Moment, now: Moment): string | null {
  const known = new Map<SessionId, string>(before.done.map((r) => [r.id, r.status]));
  const ended = now.done.filter((r) => known.get(r.id) !== r.status);
  if (ended.length === 0 && now.waiting <= before.waiting) return null;
  const parts = ended.slice(0, NAMED).map((r) => `${cut(r.name)} ${verb(r.status)}`);
  if (ended.length > NAMED) parts.push(`${ended.length - NAMED} more ended`);
  if (now.waiting > 0) parts.push(now.waiting === 1 ? "1 thing waits for you" : `${now.waiting} things wait for you`);
  return parts.length ? `While you were away, ${list(parts)}.` : null;
}

function verb(status: string): string {
  if (status === "Done") return "finished";
  if (status === "Stopped") return "stopped";
  if (status.startsWith("Failed")) return "failed";
  return "ended";
}

function cut(name: string): string {
  return name.length > NAME_CHARS ? `${name.slice(0, NAME_CHARS - 1)}…` : name;
}

function list(parts: string[]): string {
  return parts.length === 1 ? (parts[0] ?? "") : `${parts.slice(0, -1).join(", ")} and ${parts.at(-1)}`;
}
