// The git panel's agent actions (docs/specs/git-agent-actions.md): one
// read-only research session per action, whose answer becomes a proposal
// in the panel. The agent only proposes; every change to the repository is
// the person's press, made with the same git.* calls as by hand.

import { SvelteMap } from "svelte/reactivity";
import { agentSessions } from "../agents/sessions.svelte";
import type {
  AgentMessage,
  AgentStatus,
  CoreEvent,
  FileDiff,
  ProjectId,
  RepoStatus,
  SessionId,
  StatusFile,
} from "../lib/protocol";
import type { Transport } from "../lib/transport";
import {
  type AssistAction,
  commitsPrompt,
  conflictsPrompt,
  fitPrompt,
  MAX_DIFFED,
  messagePrompt,
  type PlannedCommit,
  plainLines,
  pulledPrompt,
  readMessage,
  readPlan,
} from "./assist";
import { fileLists, type GitModel, message } from "./model.svelte";

export type AssistState =
  | { kind: "idle" }
  /** Gathering the diffs, or starting the agent. */
  | { kind: "starting"; action: AssistAction }
  | { kind: "running"; action: AssistAction; session: SessionId }
  /**
   * Suggested commits, editable until Commit all. `left`: changed files no commit takes;
   * `partly`: files staged in part, which Commit all stages whole; `at`: the count of
   * repository changes seen when it was made; `confirm`: Commit all was pressed once
   * on changes that moved on since.
   */
  | {
      kind: "plan";
      commits: PlannedCommit[];
      left: string[];
      partly: string[];
      seen: string;
      at: number;
      committing: boolean;
      confirm: boolean;
    }
  | { kind: "text"; action: "pulled" | "conflicts"; lines: string[] }
  | { kind: "error"; action: AssistAction; message: string };

/** What the panel shows while an action runs. */
export const RUNNING: Record<AssistAction, string> = {
  commits: "Suggesting commits…",
  message: "Writing a commit message…",
  pulled: "Reading the new commits…",
  conflicts: "Reading the conflicts…",
};

/** The card's title for a text answer. */
export const TEXT_TITLE: Record<"pulled" | "conflicts", string> = {
  pulled: "What's new",
  conflicts: "About the conflicts",
};

/** Which files are changed and how: a plan made for other changes says so. */
export function fingerprint(files: readonly StatusFile[]): string {
  return JSON.stringify(files.map((f) => [f.path, f.staged, f.unstaged]));
}

const live = new SvelteMap<ProjectId, Assist>();

/** The agent actions of `project`'s git panel, while the panel is mounted. */
export function assistFor(project: ProjectId): Assist | undefined {
  return live.get(project);
}

export class Assist {
  state = $state<AssistState>({ kind: "idle" });
  /** A drafted commit message for the commit box to take. */
  draft = $state<string | null>(null);
  /** Grows with each new result (a plan, an answer, an error), for the card to take focus once. */
  result = $state(0);
  /** Counts the repository changes the panel heard of (not those Commit all makes). */
  private changes = $state(0);

  /** Grows with each run and stop: an older run that wakes up after an await gives up. */
  private run_ = 0;
  /** Events heard while `agents.start` runs, before the session's id is known. */
  private early: CoreEvent[] | null = null;
  private answer = "";
  private planSeen = "";
  private planAt = 0;
  private partly: string[] = [];
  private changed: string[] = [];
  private readonly off: () => void;

  constructor(
    private readonly transport: Transport,
    private readonly model: GitModel,
  ) {
    this.off = transport.onEvent((event) => this.apply(event));
    live.set(model.project, this);
  }

  /** The panel goes away: a running agent is stopped, and a start still on its way is dropped. */
  dispose(): void {
    this.off();
    this.stop();
    if (live.get(this.model.project) === this) live.delete(this.model.project);
  }

  get busy(): boolean {
    return this.state.kind === "starting" || this.state.kind === "running";
  }

  /** The actions that apply to `status`, in menu order. */
  static available(status: RepoStatus | null): AssistAction[] {
    if (!status) return [];
    const lists = fileLists(status.files);
    const actions: AssistAction[] = [];
    if (lists.conflicts.length) actions.push("conflicts");
    if (lists.staged.length + lists.changes.length && !lists.conflicts.length) actions.push("commits");
    if (lists.staged.length && !lists.conflicts.length) actions.push("message");
    if (status.head) actions.push("pulled");
    return actions;
  }

  /** Starts `action`; does nothing while another runs or Commit all works. */
  async run(action: AssistAction): Promise<void> {
    if (this.busy || this.model.state.kind !== "ready") return;
    if (this.state.kind === "plan" && this.state.committing) return;
    const run = ++this.run_;
    const status = this.model.state.status;
    this.state = { kind: "starting", action };
    this.model.notice = RUNNING[action];
    this.answer = "";
    const seen = fingerprint(status.files);
    const at = this.changes;
    let prompt: string | null;
    try {
      prompt = await this.prompt(action, status);
    } catch (err) {
      if (run === this.run_) this.fail(action, `Could not read the changes: ${message(err)}`);
      return;
    }
    if (run !== this.run_) return;
    if (prompt === null) {
      this.result++;
      this.state = {
        kind: "text",
        action: "pulled",
        lines: ["No commits came in since the last pull, merge or rebase."],
      };
      return;
    }
    this.early = [];
    let session: SessionId;
    try {
      const reply = await this.transport.call({
        method: "agents.start",
        params: {
          project: this.model.project,
          agent: "claude",
          mode: "research",
          prompt: fitPrompt(prompt),
          account: null,
          // Prompts carry other people's text (pulled commits, diffs): no web search to send anything out.
          no_web: true,
        },
      });
      session = reply.data.id;
      // The panel shows what it found: the session stays out of the Agents list.
      agentSessions(this.transport).hide(this.model.project, session);
    } catch (err) {
      this.early = null;
      if (run === this.run_) this.fail(action, `The agent did not start: ${message(err)}`);
      return;
    }
    const early = this.early ?? [];
    this.early = null;
    if (run !== this.run_) {
      // Stopped while it started.
      void this.stopSession(session);
      return;
    }
    this.state = { kind: "running", action, session };
    this.planSeen = seen;
    this.planAt = at;
    const lists = fileLists(status.files);
    this.changed = status.files.filter((f) => f.unstaged !== "conflicted").map((f) => f.path);
    this.partly = lists.staged.filter((f) => f.unstaged !== null).map((f) => f.path);
    for (const event of early) this.apply(event);
  }

  /** Stops the running action; nothing it found is kept. */
  stop(): void {
    const state = this.state;
    this.run_++;
    this.early = null;
    if (state.kind === "running") void this.stopSession(state.session);
    if (this.busy) this.state = { kind: "idle" };
  }

  dismiss(): void {
    if (this.state.kind === "plan" && this.state.committing) return;
    this.state = { kind: "idle" };
  }

  /** Changes one suggested commit's message before committing. */
  edit(index: number, text: string): void {
    if (this.state.kind !== "plan" || this.state.committing) return;
    const commits = this.state.commits.map((c, i) => (i === index ? { ...c, message: text } : c));
    this.state = { ...this.state, commits };
  }

  /** Takes one suggestion out; its files stay as changes. */
  drop(index: number): void {
    if (this.state.kind !== "plan" || this.state.committing) return;
    const dropped = this.state.commits[index];
    const commits = this.state.commits.filter((_, i) => i !== index);
    if (!commits.length) {
      this.state = { kind: "idle" };
      this.model.notice = "No suggested commits are left.";
      return;
    }
    this.state = { ...this.state, commits, left: [...this.state.left, ...(dropped?.files ?? [])] };
  }

  /** Whether the changes moved on since the plan was made: other files, or more edits to them. */
  get stale(): boolean {
    const state = this.state;
    if (state.kind !== "plan" || state.committing) return false;
    const files = this.model.state.kind === "ready" ? this.model.state.status.files : [];
    return state.at !== this.changes || state.seen !== fingerprint(files);
  }

  /**
   * Makes the suggested commits in order, as the person: for each, unstage
   * what else is staged, stage its files, commit. On changes that moved on
   * since, the first press only asks to press again. Stops at the first
   * step that fails (a hook, say), keeping the commits not made yet.
   */
  async commitAll(): Promise<boolean> {
    if (this.state.kind !== "plan" || this.state.committing || this.model.busy) return false;
    if (this.state.commits.some((c) => !c.message.trim())) return false;
    if (this.stale && !this.state.confirm) {
      this.state = { ...this.state, confirm: true };
      return false;
    }
    this.state = { ...this.state, committing: true, confirm: false };
    let made = 0;
    while (this.state.kind === "plan" && this.state.commits.length) {
      const [next, ...rest] = this.state.commits;
      if (!next) break;
      const done = await this.makeCommit(next);
      if (done === false) {
        if (this.state.kind === "plan") this.state = { ...this.state, committing: false };
        // A step refused without saying why (the panel was busy): say it here.
        if (!this.model.error) this.model.notice = "Commit all stopped before it finished. Press it again.";
        // What is left is a plan for the changes as they are now.
        if (this.state.kind === "plan") this.state = { ...this.state, at: this.changes, seen: this.seenNow() };
        return false;
      }
      if (done === "made") made++;
      if (this.state.kind === "plan") this.state = { ...this.state, commits: rest };
    }
    this.state = { kind: "idle" };
    this.model.notice =
      made === 0 ? "Nothing was left to commit." : `Made ${made} ${made === 1 ? "commit" : "commits"}.`;
    return true;
  }

  private seenNow(): string {
    return fingerprint(this.model.state.kind === "ready" ? this.model.state.status.files : []);
  }

  private async makeCommit(commit: PlannedCommit): Promise<"made" | "skipped" | false> {
    const status = () => (this.model.state.kind === "ready" ? this.model.state.status : null);
    const mine = new Set(commit.files);
    const others = fileLists(status()?.files ?? [])
      .staged.map((f) => f.path)
      .filter((p) => !mine.has(p));
    if (others.length && !(await this.model.unstage(others))) return false;
    // Files with nothing left to stage (committed or reverted meanwhile) are skipped.
    const changed = new Set((status()?.files ?? []).map((f) => f.path));
    const files = commit.files.filter((f) => changed.has(f));
    if (!files.length) return "skipped";
    if (!(await this.model.stage(files))) return false;
    const staged = fileLists(status()?.files ?? []).staged.map((f) => f.path);
    if (!staged.length) return "skipped";
    // Something else was staged meanwhile: do not commit it under the agent's message.
    if (staged.some((p) => !mine.has(p))) return false;
    return (await this.model.commit(commit.message, false)) ? "made" : false;
  }

  private async prompt(action: AssistAction, status: RepoStatus): Promise<string | null> {
    const project = this.model.project;
    const lists = fileLists(status.files);
    if (action === "conflicts") return conflictsPrompt(lists.conflicts, status.operation);
    if (action === "pulled") {
      const { data } = await this.transport.call({ method: "git.pulled", params: { project } });
      return data.commits.length ? pulledPrompt(data) : null;
    }
    const files = action === "message" ? lists.staged : status.files.filter((f) => f.unstaged !== "conflicted");
    const [log, diffs] = await Promise.all([
      this.transport
        .call({ method: "git.log", params: { project, limit: 10 } })
        .then((r) => r.data.commits)
        .catch(() => []),
      this.diffs(files, action === "message"),
    ]);
    return action === "message" ? messagePrompt(files, diffs, log) : commitsPrompt(files, diffs, log);
  }

  /** The staged and, unless `stagedOnly`, unstaged diff of each file; a file that fails is left out. */
  private async diffs(files: readonly StatusFile[], stagedOnly: boolean): Promise<FileDiff[]> {
    const asks = files
      .slice(0, MAX_DIFFED)
      .flatMap((f) => [
        ...(f.staged ? [{ path: f.path, staged: true }] : []),
        ...(!stagedOnly && f.unstaged ? [{ path: f.path, staged: false }] : []),
      ]);
    const found = await Promise.all(
      asks.map((ask) =>
        this.transport
          .call({ method: "git.diff", params: { project: this.model.project, ...ask } })
          .then((r) => r.data)
          .catch(() => null),
      ),
    );
    return found.filter((d): d is FileDiff => d !== null);
  }

  private apply(event: CoreEvent): void {
    const mine =
      (event.type === "fs.git_changed" || event.type === "fs.changed") && event.data.project === this.model.project;
    // Commit all's own steps do not make its plan stale.
    if (mine && !(this.state.kind === "plan" && this.state.committing)) this.changes++;
    if (event.type !== "agents.message" && event.type !== "agents.status") return;
    if (this.early) {
      if (this.early.length < 1000) this.early.push(event);
      return;
    }
    const state = this.state;
    if (state.kind !== "running" || event.data.session !== state.session) return;
    if (event.type === "agents.message") this.take(event.data.message);
    else this.status(state.action, event.data.status);
  }

  private status(action: AssistAction, status: AgentStatus): void {
    if (status.state === "failed") this.fail(action, `The agent stopped: ${status.message}`);
    else if (status.state === "stopped") this.fail(action, "The agent stopped before it answered.");
  }

  private take(msg: AgentMessage): void {
    const state = this.state;
    if (state.kind !== "running") return;
    if (msg.kind === "assistant_text" && !msg.parent) this.answer = msg.text;
    else if (msg.kind === "error") this.fail(state.action, `The agent stopped: ${msg.message}`);
    else if (msg.kind === "interrupted") this.fail(state.action, "The agent was stopped before it answered.");
    else if (msg.kind === "turn_end") {
      void this.stopSession(state.session);
      this.finish(state.action, this.answer);
    }
  }

  private finish(action: AssistAction, answer: string): void {
    this.result++;
    if (action === "commits") {
      const plan = readPlan(answer, this.changed);
      this.state = plan.ok
        ? {
            kind: "plan",
            commits: plan.commits,
            left: plan.left,
            partly: this.partly.filter((p) => plan.commits.some((c) => c.files.includes(p))),
            seen: this.planSeen,
            at: this.planAt,
            committing: false,
            confirm: false,
          }
        : {
            kind: "error",
            action,
            message: `The agent's suggestion could not be used: ${plan.why} Try again, or commit by hand.`,
          };
    } else if (action === "message") {
      const text = readMessage(answer);
      if (text === null) {
        this.state = { kind: "error", action, message: "The agent's answer had no commit message." };
      } else {
        this.draft = text;
        this.state = { kind: "idle" };
      }
    } else {
      const lines = plainLines(answer);
      this.state = lines.length
        ? { kind: "text", action, lines }
        : { kind: "error", action, message: "The agent gave no answer." };
    }
  }

  private fail(action: AssistAction, text: string): void {
    const state = this.state;
    this.run_++;
    if (state.kind === "running") void this.stopSession(state.session);
    this.result++;
    this.state = { kind: "error", action, message: text };
  }

  private async stopSession(session: SessionId): Promise<void> {
    try {
      await this.transport.call({ method: "agents.stop", params: { session } });
    } catch {
      // Already ended: nothing to stop.
    }
  }
}
