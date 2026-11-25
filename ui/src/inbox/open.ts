// Opens the review inbox on what a person pressed Review for: the waiting
// item of the first of the given sessions that has one. The mounted inbox
// answers; without one, nothing opens.

import type { SessionId } from "../lib/protocol";

let handler: ((sessions: SessionId[]) => void) | null = null;

/** Called by the inbox while it is mounted; returns the way to stop. */
export function handleOpenFor(open: (sessions: SessionId[]) => void): () => void {
  handler = open;
  return () => {
    if (handler === open) handler = null;
  };
}

/** Whether an inbox took the request. */
export function openInboxFor(sessions: SessionId[]): boolean {
  if (!handler) return false;
  handler(sessions);
  return true;
}
