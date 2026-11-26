// What Focus shows of a project's agents (docs/specs/focus-mode.md): rows
// for what is in progress, what comes next and what is done, and the
// sessions whose changes wait for review. Words come from the agents'
// own status labels, so Focus and the Agents panel say the same thing.

import type { AgentSessions } from "../agents/sessions.svelte";
import { isBusy } from "../agents/status";
import type { AgentStatus, ProjectId, SessionId } from "../lib/protocol";

export type FocusRow = { id: SessionId; name: string; status: string };

export type FocusSections = {
  progress: FocusRow[];
  next: FocusRow[];
  done: FocusRow[];
  /** Sessions whose changes wait for the person's review, oldest first. */
  reviews: SessionId[];
};

type Part = "progress" | "next" | "done";

export function focusSections(sessions: AgentSessions, project: ProjectId): FocusSections {
  const out: FocusSections = { progress: [], next: [], done: [], reviews: [] };
  for (const info of sessions.shown(project)) {
    out[part(sessions, info.id, info.status)].push({
      id: info.id,
      name: sessions.name(info.id),
      status: sessions.status(info.id),
    });
    if (sessions.waitsForReview(info.id)) out.reviews.push(info.id);
  }
  return out;
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

function part(sessions: AgentSessions, id: SessionId, status: AgentStatus): Part {
  if (status.state === "queued" || status.state === "preparing") return "next";
  if (isBusy(status)) return "progress";
  // A task's coordinator that waits for its workers is still on it.
  if (sessions.roles.get(id)?.kind === "coordinator") {
    const workers = sessions.workers(id).map((w) => sessions.byId.get(w)?.status ?? null);
    if (workers.some((s) => isBusy(s) || s?.state === "queued" || s?.state === "preparing")) return "progress";
  }
  return "done";
}
