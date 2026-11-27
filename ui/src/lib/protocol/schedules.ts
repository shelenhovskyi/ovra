// Mirrors crates/core/src/protocol/schedules.rs (ADR 0017).

import type { AgentKind, SessionId } from "./agents";
import type { ProjectId } from "./projects";

export type ScheduleId = string;

/** What a run starts: a research session, or one agent in its own copy whose changes wait in Reviews. */
export type ScheduleKind = "research" | "own_copy";

export type ScheduleTime = {
  /** ISO weekdays, 1 (Monday) to 7 (Sunday). */
  days: number[];
  /** 24-hour `HH:MM`. */
  time: string;
  /** The IANA time zone `time` is in. */
  time_zone: string;
};

export type ScheduleDraft = {
  name: string;
  prompt: string;
  kind: ScheduleKind;
  agent: AgentKind;
  when: ScheduleTime;
};

export type RunOutcome =
  | { outcome: "started"; session: SessionId }
  | { outcome: "skipped"; reason: string }
  | { outcome: "missed" }
  | { outcome: "failed"; message: string };

export type LastRun = { at_ms: number } & RunOutcome;

export type Schedule = ScheduleDraft & {
  id: ScheduleId;
  project: ProjectId;
  paused: boolean;
  /** The next run, ms since the Unix epoch; null while paused or switched off. */
  next_ms: number | null;
  last: LastRun | null;
  last_session: SessionId | null;
};

export type SchedulesRequest =
  | { method: "schedules.enabled" }
  | { method: "schedules.set_enabled"; params: { enabled: boolean } }
  | { method: "schedules.list"; params: { project: ProjectId } }
  | { method: "schedules.save"; params: { project: ProjectId; id?: ScheduleId; draft: ScheduleDraft } }
  | { method: "schedules.set_paused"; params: { id: ScheduleId; paused: boolean } }
  | { method: "schedules.remove"; params: { id: ScheduleId } }
  /** The person's run: also while paused or switched off; the skip rules hold. */
  | { method: "schedules.run_now"; params: { id: ScheduleId } };

export type SchedulesResponse =
  | { type: "schedules.enabled"; data: { enabled: boolean } }
  | { type: "schedules.list"; data: Schedule[] }
  | { type: "schedules.schedule"; data: Schedule };

export type SchedulesResponseFor<M extends SchedulesRequest["method"]> = M extends
  | "schedules.enabled"
  | "schedules.set_enabled"
  ? Extract<SchedulesResponse, { type: "schedules.enabled" }>
  : M extends "schedules.list"
    ? Extract<SchedulesResponse, { type: "schedules.list" }>
    : M extends "schedules.remove"
      ? { type: "ok" }
      : Extract<SchedulesResponse, { type: "schedules.schedule" }>;

/** A schedule changed or ran (`project`), or they were switched on or off (`project: null`). */
export type SchedulesEvent = { type: "schedules.changed"; data: { project: ProjectId | null } };
