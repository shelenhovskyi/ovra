// Mirrors crates/core/src/protocol/setup.rs.

import type { AgentKind, AgentSetup } from "./agents";
import type { GhStatus } from "./pr";

export type SetupRequest =
  /**
   * Checks every agent provider, git, the sandbox and, with a project, `gh`
   * for the host of the project's `origin`.
   */
  { method: "setup.check"; params: { project?: string | null } };

export type SetupResponse = { type: "setup.report"; data: SetupReport };

export type SetupReport = {
  /** Every provider the runtime can start, Claude first. */
  agents: AgentCheck[];
  git: Probe<GitTool>;
  /** `skipped` without a project: sign-in is per host, the one `origin` names. */
  gh: Probe<GhStatus>;
  sandbox: SandboxCheck;
};

export type AgentCheck = { id: AgentKind; name: string; setup: AgentSetup };

export type GitTool = { installed: boolean; version: string | null };

/** What a check found, why it could not be made, or that it was not made. */
export type Probe<T> = { state: "checked"; found: T } | { state: "failed"; message: string } | { state: "skipped" };

export type SandboxCheck =
  | { state: "available"; name: string; gaps: string[] }
  | { state: "unavailable"; reason: string };
