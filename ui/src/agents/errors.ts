// What went wrong in a session's last turn, and the transcript without the
// agent's own copy of an error.

import type { AgentMessage, AgentStatus } from "../lib/protocol";

/** How the runtime words a turn the person interrupted; that is no failure. */
const INTERRUPTED = "Interrupted.";

const AUTH_FAILURE =
  /failed to authenticate|oauth (session|token)|not logged in|please run \/login|invalid api key|authentication_error/i;

/**
 * Whether claude's error says its account is not signed in, such as
 * "Failed to authenticate: OAuth session expired and could not be refreshed".
 * Ovra never sees the sign-in itself (ADR 0006); claude's words are all it has.
 */
export function isAuthFailure(text: string): boolean {
  return AUTH_FAILURE.test(text);
}

/** The error the last turn ended with, if it failed; an interrupt is none. */
function turnError(messages: AgentMessage[] | undefined): string | null {
  for (let i = (messages?.length ?? 0) - 1; i >= 0; i--) {
    const message = messages?.[i];
    if (message?.kind === "error") return message.message.trim() === INTERRUPTED ? null : message.message;
    // A message to the agent starts a turn: anything before it is an older turn's.
    if (message?.kind === "user_text") return null;
  }
  return null;
}

/** Why the session cannot go on because its account is signed out; null when it can. */
export function signedOutError(status: AgentStatus, messages: AgentMessage[] | undefined): string | null {
  if (status.state === "failed") return isAuthFailure(status.message) ? status.message : null;
  if (status.state !== "idle" && status.state !== "stopped") return null;
  const error = turnError(messages);
  return error !== null && isAuthFailure(error) ? error : null;
}

/**
 * A failed turn's status: claude ends it like any other turn, so "idle"
 * alone would read as done. Null when the plain label is right.
 */
export function failedLabel(status: AgentStatus, messages: AgentMessage[] | undefined): string | null {
  if (signedOutError(status, messages) !== null) return "Failed: signed out";
  if (status.state === "idle" && turnError(messages) !== null) return "Failed";
  return null;
}

/**
 * The transcript without an agent message that only repeats the error its
 * turn ended with: claude says the error as text and as its result.
 */
export function withoutEchoes(messages: AgentMessage[]): AgentMessage[] {
  const echoes = new Set<number>();
  let turnErrors: string[] = [];
  // Backwards, so each message knows the errors later in its own turn.
  for (let i = messages.length - 1; i >= 0; i--) {
    const message = messages[i];
    if (message?.kind === "user_text") turnErrors = [];
    else if (message?.kind === "error") turnErrors.push(message.message.trim());
    else if (message?.kind === "assistant_text" && !message.parent && turnErrors.includes(message.text.trim()))
      echoes.add(i);
  }
  return echoes.size === 0 ? messages : messages.filter((_, i) => !echoes.has(i));
}
