// Mirrors crates/core/src/protocol/resources.rs.

import type { SessionId } from "./agents";
import type { ProjectId } from "./projects";

/** What one process tree takes: a root process and every descendant. */
export type Usage = {
  /** Resident memory summed over the tree; an upper bound. */
  memory_bytes: number;
  /** CPU since the previous sample; 100 is one core busy. */
  cpu_percent: number;
  processes: number;
};

/** One session's share; a part is null while it does not run. */
export type SessionResources = {
  session: SessionId;
  /** The agent's process and every command it runs. */
  agent: Usage | null;
  /** The dev servers of an isolated session's worktree. */
  servers: Usage | null;
  browser: Usage | null;
};

export type SystemLoad = {
  memory_total_bytes: number;
  memory_available_bytes: number;
  /** How busy all cores are together, 0 to 100. */
  cpu_percent: number;
};

/** Why queued workers wait although a place may be free. */
export type Pressure = "low_memory" | "busy_cpu";

export type ResourcesEvent = {
  /** Every few seconds per project while any of its sessions has not ended. */
  type: "resources.sample";
  data: { project: ProjectId; system: SystemLoad; pressure: Pressure | null; sessions: SessionResources[] };
};
