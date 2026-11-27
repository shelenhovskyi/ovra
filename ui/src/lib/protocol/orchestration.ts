import type { AgentKind, AgentModel, SessionId } from "./agents";
import type { ContextItem } from "./context";
import type { ProjectId } from "./projects";
import type { ChangeKind, ReviewId } from "./review";

export type TaskId = string;

/** A worker's `push` waiting for the person: the `input` of a permission request whose `tool` is `Push` (`docs/specs/agent-push.md`). */
export type PushPlan = {
  /** The remote's name, such as `origin`. */
  remote: string;
  /** The host its URL names, if Ovra could read it. */
  host?: string;
  branch: string;
  /** The remote has no such branch yet: it starts from `base`. */
  new_branch: boolean;
  /** What a new branch starts from, on the same remote. */
  base?: string;
  /** The commit the push builds on. */
  onto: string;
  /** The one commit Ovra made of the worker's changes. */
  commit: string;
  /** Written by the agent. */
  message: string;
  /** The files the commit changes on top of `onto`. */
  files: PushedFile[];
  /** A shared branch such as `main`: only a rule naming it exactly pushes to it without asking. */
  protected_branch: boolean;
  /** The worker's own branch; any other is pushed to without asking only by a rule naming it exactly. */
  own_branch?: boolean;
};

export type PushedFile = {
  path: string;
  kind: ChangeKind;
  from?: string;
  additions: number;
  deletions: number;
};

export type TaskLimits = {
  /** Workers that may run at once (default 3). */
  max_workers: number;
  /** Spending cap for all of the task's sessions, in millionths of a dollar. */
  budget_micro_usd: number | null;
};

export type Worker = {
  session: SessionId;
  /** The label the coordinator gave it. */
  name: string;
  branch: string;
  /** Absolute path of its worktree. */
  worktree: string;
  /** Why its worktree stays although its task ended (for example ignored files such as `.env`). */
  kept: string | null;
  /** Its worktree's setup while it runs and after it failed; null without setup or once it succeeded (absent from older cores). */
  setup?: SetupProgress | null;
  /** Its worktree's size on disk; null until counted (absent from older cores). */
  disk?: DiskUsage | null;
  /** Why it waits queued although a place may be free (too little disk space for its worktree); absent from older cores. */
  waiting?: string | null;
  /** Paths copied into its worktree through `.worktreeinclude`, relative, shallowest first, at most 100 (absent from older cores). */
  included?: string[];
};

/** Where a worktree's setup steps (`.ovra/worktree.json`) are. */
export type SetupProgress = {
  /** The step that runs, or that failed, from 0. */
  step: number;
  steps: number;
  /** That step's program and arguments, for people. */
  command: string;
  /** The last lines of its output (at most 50). */
  log: string[];
  /** Why it failed or timed out; null while it runs. */
  failed: string | null;
};

/** A folder's size on disk. */
export type DiskUsage = {
  bytes: number;
  /** true: all counted, an upper bound ("up to"); false: counting stopped early ("more than"). */
  complete: boolean;
};

/** A folder under Ovra's worktrees folder that no task knows; kept until the person removes it. */
export type Leftover = {
  /** Absolute path. */
  path: string;
  /** null when it could not be counted. */
  disk: DiskUsage | null;
};

/** A worker the coordinator asked for that could not start. */
export type FailedWorker = {
  /** The label the coordinator gave it. */
  name: string;
  /** Why it did not start, in words for people. */
  reason: string;
};

export type Task = {
  id: TaskId;
  project: ProjectId;
  /** `null` for one agent in its own copy (`solo`). */
  coordinator: SessionId | null;
  workers: Worker[];
  /** Workers that never started, so they have no session to show (absent from older cores). */
  failed?: FailedWorker[];
  limits: TaskLimits;
  spent_micro_usd: number;
  /** Another attempt at that solo task's prompt (`docs/specs/attempts.md`); absent otherwise. */
  attempt_of?: TaskId;
};

export type ShowTarget =
  | { kind: "file"; path: string; line: number | null }
  | { kind: "agent"; session: SessionId }
  | { kind: "review"; id: ReviewId };

export type OrchestrationRequest =
  | {
      method: "orchestration.start";
      params: {
        project: ProjectId;
        prompt: string;
        limits?: TaskLimits;
        context?: ContextItem[];
        /** The provider that works on it; `claude` when left out. */
        agent?: AgentKind;
        /** One agent in its own worktree, without a coordinator. */
        solo?: boolean;
        /** With `solo`: one more attempt at that first solo task, at most three per prompt. */
        attempt_of?: TaskId;
        /** Claude only: the model its sessions ask for; a coordinator's workers use their coordinator's. */
        model?: AgentModel;
        /** With `solo`: proposes a plan first and changes no file until the person approves it. */
        plan_first?: boolean;
      };
    }
  | { method: "orchestration.list"; params: { project: ProjectId } }
  | { method: "orchestration.stop"; params: { task: TaskId } }
  /** Retry, or with `skip_setup` Start anyway, a worker whose setup failed. */
  | { method: "orchestration.retry_setup"; params: { session: SessionId; skip_setup?: boolean } }
  | { method: "orchestration.leftovers"; params: { project: ProjectId } }
  | { method: "orchestration.remove_leftover"; params: { project: ProjectId; path: string } };

export type OrchestrationResponse =
  | { type: "orchestration.task"; data: Task }
  | { type: "orchestration.list"; data: Task[] }
  | { type: "orchestration.leftovers"; data: Leftover[] };

export type OrchestrationEvent =
  | { type: "orchestration.changed"; data: { project: ProjectId; task: TaskId } }
  /** An agent asked to show something (`open_in_ui`). */
  | { type: "orchestration.show"; data: { project: ProjectId; target: ShowTarget } };
