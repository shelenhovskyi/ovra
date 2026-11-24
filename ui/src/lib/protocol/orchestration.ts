import type { SessionId } from "./agents";
import type { ProjectId } from "./projects";
import type { ReviewId } from "./review";

export type TaskId = string;

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
};

export type Task = {
  id: TaskId;
  project: ProjectId;
  coordinator: SessionId;
  workers: Worker[];
  limits: TaskLimits;
  spent_micro_usd: number;
};

export type ShowTarget =
  | { kind: "file"; path: string; line: number | null }
  | { kind: "agent"; session: SessionId }
  | { kind: "review"; id: ReviewId };

export type OrchestrationRequest =
  | { method: "orchestration.start"; params: { project: ProjectId; prompt: string; limits?: TaskLimits } }
  | { method: "orchestration.list"; params: { project: ProjectId } }
  | { method: "orchestration.stop"; params: { task: TaskId } };

export type OrchestrationResponse =
  | { type: "orchestration.task"; data: Task }
  | { type: "orchestration.list"; data: Task[] };

export type OrchestrationEvent =
  | { type: "orchestration.changed"; data: { project: ProjectId; task: TaskId } }
  /** An agent asked to show something (`open_in_ui`). */
  | { type: "orchestration.show"; data: { project: ProjectId; target: ShowTarget } };
