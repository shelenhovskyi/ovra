// Mirrors crates/core/src/protocol/policy.rs.

import type { ProjectId } from "./projects";

/** `auto` skips the question only in a sandboxed session with a limited network. */
export type PolicyAction = "auto" | "ask" | "deny";

export type PolicyRule = {
  /** A tool name, such as `Bash` or `Edit`. */
  tool: string;
  /**
   * A command prefix for Bash, a glob on the session-relative path for file tools, the URL's host for
   * WebFetch; absent matches every call.
   */
  match?: string;
  action: PolicyAction;
  /** Added by a person's "Always allow" on an approval, not in Settings. */
  from_approval?: true;
};

/** A project's rules; the empty policy asks about everything. */
export type Policy = { rules: PolicyRule[] };

export type PolicyRequest =
  | { method: "policy.get"; params: { project: ProjectId } }
  /**
   * Replaces the policy; `bad_request` for rules that cannot mean what they say.
   * With `expected` (the policy the change was built from), `conflict` and nothing
   * kept when the stored policy differs.
   */
  | { method: "policy.set"; params: { project: ProjectId; policy: Policy; expected?: Policy } };

export type PolicyResponse = { type: "policy.policy"; data: Policy };

export type PolicyEvent = { type: "policy.changed"; data: { project: ProjectId } };
