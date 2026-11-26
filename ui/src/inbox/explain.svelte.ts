// "Explain these changes" in Reviews: one read-only research session per
// press (ADR 0013), whose answer is shown beside the diff. The agent only
// explains; nothing it says keeps or discards anything.

import { SvelteMap } from "svelte/reactivity";
import { agentSessions } from "../agents/sessions.svelte";
import type { AgentMessage, AgentStatus, CoreEvent, ReviewId, ReviewItem, SessionId } from "../lib/protocol";
import type { Transport } from "../lib/transport";
import { type Explanation, explainedFor, explainPrompt, MAX_EXPLAINED, readExplanation } from "./explain";
import { message } from "./reviews.svelte";

export type ExplainState =
  /** Reading the diffs, or starting the agent. */
  | { kind: "starting" }
  | { kind: "running"; session: SessionId }
  /** `seen`: the item's files when it was asked (`explainedFor`). */
  | { kind: "done"; explanation: Explanation; seen: string }
  | { kind: "error"; message: string };

type Run = {
  id: ReviewId;
  session: SessionId | null;
  /** Events heard while `agents.start` runs, before the session's id is known. */
  early: CoreEvent[] | null;
  answer: string;
  seen: string;
  paths: string[];
};

export class Explainer {
  /** By review item; an item never explained has none. */
  readonly states = new SvelteMap<ReviewId, ExplainState>();
  private readonly runs = new Map<ReviewId, Run>();
  private readonly off: () => void;

  constructor(
    private readonly transport: Transport,
    /** A file's diff, as Reviews reads it. */
    private readonly diff: (id: ReviewId, path: string) => Promise<{ patch: string }>,
  ) {
    this.off = transport.onEvent((event) => this.apply(event));
  }

  /** Reviews goes away: running agents are stopped, starts on their way dropped. */
  dispose(): void {
    this.off();
    for (const id of [...this.runs.keys()]) this.stop(id);
  }

  state(id: ReviewId): ExplainState | null {
    return this.states.get(id) ?? null;
  }

  busy(id: ReviewId): boolean {
    const kind = this.states.get(id)?.kind;
    return kind === "starting" || kind === "running";
  }

  /** Whether `item`'s files changed since its explanation was written. */
  stale(item: ReviewItem): boolean {
    const state = this.states.get(item.id);
    return state?.kind === "done" && state.seen !== explainedFor(item);
  }

  /** Starts explaining `item`, named `task`; does nothing while one runs for it. */
  async explain(item: ReviewItem, task: string): Promise<void> {
    if (this.busy(item.id) || !item.files.length) return;
    const run: Run = {
      id: item.id,
      session: null,
      early: null,
      answer: "",
      seen: explainedFor(item),
      paths: item.files.map((f) => f.path),
    };
    this.runs.set(item.id, run);
    this.states.set(item.id, { kind: "starting" });
    const asked = item.files.slice(0, MAX_EXPLAINED);
    // A file whose diff fails is named only.
    const patches = (
      await Promise.all(
        asked.map((f) =>
          this.diff(item.id, f.path).then(
            (d) => ({ path: f.path, patch: d.patch }),
            () => null,
          ),
        ),
      )
    ).filter((p): p is { path: string; patch: string } => p !== null);
    if (!this.current(run)) return;
    run.early = [];
    let session: SessionId;
    try {
      const reply = await this.transport.call({
        method: "agents.start",
        params: {
          project: item.project,
          agent: "claude",
          mode: "research",
          prompt: explainPrompt(item, task, patches),
          account: null,
          // The diffs are the agent's text, not the person's: no web search to send them out.
          no_web: true,
        },
      });
      session = reply.data.id;
      // Reviews shows what it says: the session stays out of the Agents list.
      agentSessions(this.transport).hide(item.project, session);
    } catch (err) {
      run.early = null;
      if (this.current(run)) this.fail(run, `The agent did not start: ${message(err)}`);
      return;
    }
    const early = run.early ?? [];
    run.early = null;
    if (!this.current(run)) {
      void this.stopSession(session);
      return;
    }
    run.session = session;
    this.states.set(item.id, { kind: "running", session });
    for (const event of early) this.apply(event);
  }

  /** Stops explaining `id`; nothing it found is kept. */
  stop(id: ReviewId): void {
    const run = this.runs.get(id);
    this.runs.delete(id);
    if (run?.session) void this.stopSession(run.session);
    if (this.busy(id)) this.states.delete(id);
  }

  /** Puts the explanation of `id` away. */
  dismiss(id: ReviewId): void {
    if (!this.busy(id)) this.states.delete(id);
  }

  private current(run: Run): boolean {
    return this.runs.get(run.id) === run;
  }

  private apply(event: CoreEvent): void {
    if (event.type !== "agents.message" && event.type !== "agents.status") return;
    for (const r of this.runs.values()) if (r.early && r.early.length < 1000) r.early.push(event);
    const run = [...this.runs.values()].find((r) => r.session === event.data.session);
    if (!run) return;
    if (event.type === "agents.message") this.take(run, event.data.message);
    else this.status(run, event.data.status);
  }

  private status(run: Run, status: AgentStatus): void {
    if (status.state === "failed") this.fail(run, `The agent stopped: ${status.message}`);
    else if (status.state === "stopped") this.fail(run, "The agent stopped before it answered.");
  }

  private take(run: Run, msg: AgentMessage): void {
    if (msg.kind === "assistant_text" && !msg.parent) run.answer = msg.text;
    else if (msg.kind === "error") this.fail(run, `The agent stopped: ${msg.message}`);
    else if (msg.kind === "interrupted") this.fail(run, "The agent was stopped before it answered.");
    else if (msg.kind === "turn_end") {
      this.runs.delete(run.id);
      if (run.session) void this.stopSession(run.session);
      const explanation = readExplanation(run.answer, run.paths);
      this.states.set(
        run.id,
        explanation
          ? { kind: "done", explanation, seen: run.seen }
          : { kind: "error", message: "The agent gave no answer." },
      );
    }
  }

  private fail(run: Run, text: string): void {
    this.runs.delete(run.id);
    if (run.session) void this.stopSession(run.session);
    this.states.set(run.id, { kind: "error", message: text });
  }

  private async stopSession(session: SessionId): Promise<void> {
    try {
      await this.transport.call({ method: "agents.stop", params: { session } });
    } catch {
      // Already ended: nothing to stop.
    }
  }
}
