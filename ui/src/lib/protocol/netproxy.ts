// Mirrors crates/core/src/protocol/netproxy.rs.

import type { SessionId } from "./agents";
import type { ProjectId } from "./projects";

/** A lowercase DNS name or IP address; without a port it allows 443 and 80. */
export type AllowedHost = { host: string; port?: number };

export type Allowlist = { hosts: AllowedHost[] };

export type ProjectAllowlist = {
  /** What `claude` itself needs; not editable. */
  base: Allowlist;
  /** The project's own hosts, added to `base`. */
  project: Allowlist;
  /** Hosts in `project` where anyone can receive data; the UI warns about each. */
  broad: string[];
};

export type NetproxyRequest =
  | { method: "netproxy.get_allowlist"; params: { project: ProjectId } }
  /** Replaces the project's own hosts; `bad_request` for wildcards, port 0 or unreachable addresses. */
  | { method: "netproxy.set_allowlist"; params: { project: ProjectId; allowlist: Allowlist } };

export type NetproxyResponse = { type: "netproxy.allowlist"; data: ProjectAllowlist };

export type NetproxyEvent =
  | { type: "netproxy.changed"; data: { project: ProjectId } }
  /** `host` is `host:port` as the agent asked for it. */
  | { type: "netproxy.refused"; data: { project: ProjectId; session: SessionId; host: string; reason: string } };
