// Going back to an earlier turn of a direct session (`docs/specs/checkpoints.md`):
// which turn each message belongs to, counted as the core counts them
// (`crates/core/src/agents/transcript.rs`), which turns a go-back undid, and
// the words for it.

import type { AgentMessage } from "../lib/protocol";

/** How the note a go-back leaves for the agent's next message starts (core: `direct/turns.rs`). */
const NOTE = "The files were set back to how they were before your turn";

/**
 * Each message's turn, from 1: every run of user texts in a row (the
 * person's text and Ovra's notes) starts the next one. Messages before the
 * first user text have none.
 */
export function turnsOf(messages: AgentMessage[]): Map<AgentMessage, number> {
  const turns = new Map<AgentMessage, number>();
  let turn = 0;
  let previous: AgentMessage | undefined;
  for (const message of messages) {
    if (message.kind === "user_text" && previous?.kind !== "user_text") turn++;
    if (turn > 0) turns.set(message, turn);
    previous = message;
  }
  return turns;
}

/**
 * The turns a go-back to `from` undid. While it can still be undone, no
 * message was sent since: every turn from `from` on. Later, the message that
 * carried Ovra's note about it ends them.
 */
export function undoneTurns(
  messages: AgentMessage[],
  from: number | null | undefined,
  pending: boolean,
): { from: number; to: number } | null {
  if (!from) return null;
  const turns = turnsOf(messages);
  let latest = 0;
  let noted = 0;
  for (const [message, turn] of turns) {
    latest = turn;
    if (turn > from && message.kind === "user_text" && message.from === "ovra" && message.text.startsWith(NOTE))
      noted = turn;
  }
  const to = !pending && noted > 0 ? noted - 1 : latest;
  return to >= from ? { from, to } : null;
}

/** `turn 2` or `turns 2 to 4`. */
export function turnsText(from: number, to: number): string {
  return from === to ? `turn ${from}` : `turns ${from} to ${to}`;
}

/** What a go-back (or its undo) did, in one line. */
export function resultText(restored: string[], conflicts: string[], undo: boolean): string {
  const done =
    restored.length > 0
      ? `${undo ? "Put back" : "Set back"}: ${restored.join(", ")}.`
      : undo
        ? "Nothing to put back."
        : "Nothing to set back: no file has only the agent's changes.";
  return conflicts.length > 0 ? `${done} Kept as they are (changed since): ${conflicts.join(", ")}.` : done;
}
