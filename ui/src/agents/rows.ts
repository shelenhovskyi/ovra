// The agents panel's task list: which rows come first, and what each row
// says (its title, tooltip, call to action, state and workers' progress).
// Read from the sessions and task boards, so it is tested without the panel.

import type { TaskBoards } from "../board/boards.svelte";
import type { SessionId, SessionInfo } from "../lib/protocol";
import type { AgentSessions } from "./sessions.svelte";
import { isBusy } from "./status";

export type Chip = { text: string; tone: "warn" | "ok" };

export class TaskRows {
  constructor(
    private readonly sessions: AgentSessions,
    private readonly boards: TaskBoards,
  ) {}

  /**
   * The row's call to action, if the person is needed. A task says how many
   * of its workers' changes wait; its workers in `list` do not say it again.
   */
  chip(s: SessionInfo, list: readonly SessionInfo[]): Chip | null {
    if (s.status.state === "waiting_for_approval") return { text: "Approve", tone: "warn" };
    const role = this.sessions.roles.get(s.id);
    if (role?.kind === "coordinator" && this.boards.openForPerson(role.task).length > 0)
      return { text: "Answer", tone: "warn" };
    // A hint only: the row says so, and the person decides.
    if (s.stuck && s.status.state === "working") return { text: "Stuck?", tone: "warn" };
    if (isBusy(s.status)) return null;
    const inTask = this.sessions.waitingInTask(s.id);
    if (inTask > 0) return { text: inTask > 1 ? `Review ${inTask}` : "Review", tone: "ok" };
    const told =
      role?.kind === "worker" && list.some((t) => t.id === role.coordinator && this.chip(t, list)?.tone === "ok");
    return this.sessions.waitsForReview(s.id) && !told ? { text: "Review", tone: "ok" } : null;
  }

  /**
   * A row's title: a worker under its task by its own name and what it was
   * asked to do (its first line), a task or a session by its title.
   */
  title(s: SessionInfo): string {
    const role = this.sessions.roles.get(s.id);
    if (role?.kind === "worker") return s.title ? `${role.name} · ${s.title}` : role.name;
    return s.title || this.sessions.name(s.id);
  }

  /** The row's tooltip: the whole title, and a worker's branch and kept folder. */
  tip(s: SessionInfo): string {
    const role = this.sessions.roles.get(s.id);
    if (role?.kind !== "worker") return this.sessions.name(s.id);
    const kept = role.kept ? `\nIts folder was kept: ${role.kept}` : "";
    return `${role.name}${s.title ? `: ${s.title}` : ""}\nBranch ${role.branch}${kept}`;
  }

  /** Tasks in the order "needs you" first; a task's workers stay under it, in their order. */
  ordered(list: readonly SessionInfo[]): SessionInfo[] {
    const groups: SessionInfo[][] = [];
    for (const s of list) {
      const role = this.sessions.roles.get(s.id);
      const last = groups.at(-1);
      if (role?.kind === "worker" && last?.[0]?.id === role.coordinator) last.push(s);
      else groups.push([s]);
    }
    return groups
      .map((group, at) => ({ group, at, rank: Math.min(...group.map((s) => this.rank(s, list))) }))
      .sort((a, b) => a.rank - b.rank || a.at - b.at)
      .flatMap((g) => g.group);
  }

  /** How many of a task's workers are done (or wait for review), of how many; null without workers. */
  progress(coordinator: SessionId): { done: number; total: number } | null {
    const workers = this.sessions.workers(coordinator);
    const total = workers.length + (this.sessions.failedStarts.get(coordinator)?.length ?? 0);
    if (total === 0) return null;
    const done = workers.filter((w) => this.sessions.waitsForReview(w) || this.sessions.status(w) === "Done").length;
    return { done, total };
  }

  /** The row's colour: its state, except that failed and done follow what the row says. */
  tone(s: SessionInfo): string {
    const label = this.sessions.status(s.id);
    if (s.status.state === "failed" || label.startsWith("Failed")) return "failed";
    // A worker stopped with its task after it finished is done.
    if (label === "Done") return "idle";
    return s.status.state;
  }

  /** Needs the person first, then working, then queued, then the rest. */
  private rank(s: SessionInfo, list: readonly SessionInfo[]): number {
    if (this.chip(s, list)) return 0;
    if (isBusy(s.status)) return 1;
    return s.status.state === "queued" ? 2 : 3;
  }
}
