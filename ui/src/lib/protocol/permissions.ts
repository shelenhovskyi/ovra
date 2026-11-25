import type { SessionId } from "./agents";
import type { PolicyRule } from "./policy";
import type { ProjectId } from "./projects";

export type PermissionId = string;

export type PermissionRequest = {
  id: PermissionId;
  session: SessionId;
  /** The session's project, so a client that does not know the session still knows where it comes from. */
  project: ProjectId;
  /** The folder the session works in (the project root, or a worker's worktree); paths are judged against it. */
  cwd?: string;
  /** Tool name as the agent reports it, e.g. `Bash` or `Edit`. */
  tool: string;
  input: unknown;
  title: string | null;
  /** Why the agent's own rules ask a person, if it says. */
  reason?: PermissionReason;
  /**
   * The rule "Always allow" would add: the narrowest `auto` rule for this call. Offered only in a
   * sandboxed session with a limited network, inside its folder, never for protected paths or calls
   * a project rule asks about.
   */
  always?: PolicyRule;
};

/** Why an agent asks rather than deciding itself. */
export type PermissionReason = {
  /** For people, e.g. "This command requires approval". */
  text: string;
  /** The agent's own category, e.g. `rule`, `hook` or `other`, if given. */
  kind?: string;
};

export type Decision = "allow" | "deny";

export type PermissionsRequest =
  | { method: "permissions.pending" }
  /**
   * `message` is shown to the agent on deny. With `always` (and `allow`), the request's `always` rule
   * is added to the project's policy first; `bad_request` when there is none or a rule now asks about it.
   */
  | {
      method: "permissions.answer";
      params: { id: PermissionId; decision: Decision; message?: string; always?: boolean };
    };

export type PermissionsResponse = { type: "permissions.pending"; data: PermissionRequest[] };

export type PermissionsEvent =
  | { type: "permissions.requested"; data: PermissionRequest }
  /** Answered, or dropped as `deny` because its session ended. */
  | { type: "permissions.resolved"; data: { id: PermissionId; decision: Decision } };
