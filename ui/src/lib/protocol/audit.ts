// Mirrors crates/core/src/protocol/audit.rs: the audit log's schema and `audit.*`.

import type { SandboxReport, SessionId, SessionMode } from "./agents";
import type { ServerId as LspServerId } from "./lsp";
import type { Policy, PolicyRule } from "./policy";
import type { ProjectId } from "./projects";
import type { ReviewId } from "./review";
import type { ServerId, StartedBy } from "./servers";

export type AuditEntry = {
  seq: number;
  at_ms: number;
  project: ProjectId;
  session: SessionId | null;
  event: AuditEvent;
};

export type AuditEvent =
  | { kind: "session_started"; mode: SessionMode; trusted: boolean; sandbox: SandboxReport | null }
  | { kind: "tool_decided"; tool: string; input: AuditInput; allowed: boolean; by: DecidedBy }
  | { kind: "trust_changed"; trusted: boolean; fingerprint: string | null }
  | { kind: "policy_changed"; policy: Policy }
  /** `by` added `rule` from an approval ("Always allow"); the entry's session is the one that asked. */
  | { kind: "rule_added"; rule: PolicyRule; by: DecidedBy }
  | { kind: "review_decided"; review: ReviewId; action: ReviewAction }
  | { kind: "network_refused"; host: string; reason: string }
  | { kind: "secret_used"; name: string; host: string }
  | { kind: "server_started"; server: ServerId; name: string; generation: number; by: StartedBy }
  /** `code` is null when it was killed by a signal. */
  | { kind: "server_stopped"; server: ServerId; name: string; generation: number; code: number | null }
  /** The session may reach the server on `localhost:<port>` while this generation runs. */
  | { kind: "loopback_granted"; server: ServerId; port: number; generation: number }
  | { kind: "loopback_revoked"; server: ServerId; port: number; generation: number }
  /** A language server for the project's files; `sandbox` is null without one. */
  | { kind: "language_server_started"; server: LspServerId; name: string; language: string; sandbox: string | null }
  /** `reason` says why it failed; null when it was stopped or exited cleanly. */
  | { kind: "language_server_stopped"; server: LspServerId; name: string; language: string; reason: string | null }
  /** Whether the session's browser may load an origin that is not a granted server. */
  | { kind: "browser_navigation"; origin: string; allowed: boolean; by: NavigationBy };

/** Never file contents; other inputs redacted and cut to 4 KiB. */
export type AuditInput = { kind: "file"; path: string; sha256: string | null } | { kind: "json"; text: string };

export type DecidedBy =
  | { kind: "person" }
  | { kind: "policy"; rule: string }
  | { kind: "role" }
  | { kind: "session_ended" };

/** Who decided a browser navigation; `no_answer` and `session_ended` refuse. */
export type NavigationBy = "person" | "no_answer" | "session_ended";

export type ReviewAction = "approved" | "partly_approved" | "rejected" | "undone_reject" | "taken" | "undone_take";

/** `newest_first` without `before` opens the end of the log; `before` pages back. */
export type AuditOrder = "oldest_first" | "newest_first";

export type AuditRequest = {
  method: "audit.list";
  /**
   * Entries with a `seq` above `after` and below `before`, the first `limit` (at most 500, the default) in
   * `order` (`oldest_first` by default).
   */
  params: {
    project: ProjectId;
    after?: number | null;
    before?: number | null;
    order?: AuditOrder;
    limit?: number | null;
  };
};

export type AuditResponse = { type: "audit.entries"; data: AuditEntry[] };

export type AuditLogEvent = { type: "audit.appended"; data: AuditEntry };
