// What Focus shows of a project's agents (docs/specs/focus-mode.md): rows
// for what is in progress, what comes next and what is done, and the
// sessions whose changes wait for review. Words come from the agents'
// own status labels, so Focus and the Agents panel say the same thing.
//
// One view model for Focus and the phone companion (D5 of
// docs/research/focus-mode.md): it reads sessions through `FocusSource`, not
// the IDE's own state, and imports nothing of the IDE, so the phone's Home
// screen sorts and words agents the way Focus does. What a phone may do stays
// limited in the core (`crates/core/src/companion.rs`).

import { isBusy } from "../agents/status";
import type { AgentMessage, AgentStatus, ProjectId, SessionId, SessionInfo } from "../lib/protocol";

/** What the model reads of a project's sessions; `AgentSessions` is one. */
export interface FocusSource {
  /** The project's sessions the person has not closed. */
  shown(project: ProjectId): SessionInfo[];
  name(id: SessionId): string;
  /** What the session does now, in words. */
  status(id: SessionId): string;
  waitsForReview(id: SessionId): boolean;
  workers(coordinator: SessionId): SessionId[];
  readonly byId: ReadonlyMap<SessionId, SessionInfo>;
  readonly roles: ReadonlyMap<SessionId, { kind: string }>;
  readonly transcripts: ReadonlyMap<SessionId, AgentMessage[]>;
}

/** `id` is the session's; a planned step's row carries its session's id and its place in the plan. */
export type FocusRow = { id: SessionId; name: string; status: string; step?: number };

/** At most this many planned steps per session, each cut to this many characters. */
const MAX_STEPS = 8;
const MAX_STEP_CHARS = 160;

export type FocusSections = {
  progress: FocusRow[];
  next: FocusRow[];
  done: FocusRow[];
  /** Sessions whose changes wait for the person's review, oldest first. */
  reviews: SessionId[];
};

type Part = "progress" | "next" | "done";

export function focusSections(sessions: FocusSource, project: ProjectId): FocusSections {
  const out: FocusSections = { progress: [], next: [], done: [], reviews: [] };
  const planned: FocusRow[] = [];
  for (const info of sessions.shown(project)) {
    const where = part(sessions, info.id, info.status);
    const name = sessions.name(info.id);
    out[where].push({ id: info.id, name, status: sessions.status(info.id) });
    if (sessions.waitsForReview(info.id)) out.reviews.push(info.id);
    // What an agent at work plans to do next, from its own to-do list.
    if (where === "progress")
      for (const [step, text] of pendingSteps(sessions.transcripts.get(info.id) ?? []).entries())
        planned.push({ id: info.id, name: text, status: `Planned by ${name}`, step });
  }
  out.next.push(...planned);
  return out;
}

/**
 * The steps still pending in the agent's latest to-do list (Claude's
 * `TodoWrite`): agent-written text, shown as text, cut short. An input
 * that is not the expected shape gives none.
 */
export function pendingSteps(messages: AgentMessage[]): string[] {
  for (let i = messages.length - 1; i >= 0; i--) {
    const message = messages[i];
    if (message?.kind !== "tool_use" || message.name !== "TodoWrite") continue;
    const todos = (message.input as { todos?: unknown } | null)?.todos;
    if (!Array.isArray(todos)) return [];
    return todos
      .filter(
        (t): t is { content: string; status: string } =>
          typeof t?.content === "string" && t.status === "pending" && t.content.trim() !== "",
      )
      .slice(0, MAX_STEPS)
      .map((t) => {
        const text = t.content.trim().replace(/\s+/g, " ");
        return text.length > MAX_STEP_CHARS ? `${text.slice(0, MAX_STEP_CHARS - 1)}…` : text;
      });
  }
  return [];
}

/** "2 working · 1 next · 3 done", leaving out what is empty. */
export function countLine(sections: FocusSections): string {
  const parts = [
    sections.progress.length ? `${sections.progress.length} working` : "",
    sections.next.length ? `${sections.next.length} next` : "",
    sections.done.length ? `${sections.done.length} done` : "",
  ].filter(Boolean);
  return parts.length ? parts.join(" · ") : "No agents yet";
}

function part(sessions: FocusSource, id: SessionId, status: AgentStatus): Part {
  if (status.state === "queued" || status.state === "preparing") return "next";
  if (isBusy(status)) return "progress";
  // A task's coordinator that waits for its workers is still on it.
  if (sessions.roles.get(id)?.kind === "coordinator") {
    const workers = sessions.workers(id).map((w) => sessions.byId.get(w)?.status ?? null);
    if (workers.some((s) => isBusy(s) || s?.state === "queued" || s?.state === "preparing")) return "progress";
  }
  return "done";
}
