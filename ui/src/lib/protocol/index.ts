// Mirrors crates/core/src/protocol/. One file per domain, like the Rust side.

import type { AgentsEvent, AgentsRequest, AgentsResponse, CheckpointMethod, CheckpointResponseFor } from "./agents";
import type { AuditLogEvent, AuditRequest, AuditResponse } from "./audit";
import type { BrowserEvent, BrowserRequest, BrowserResponse, BrowserResponseFor } from "./browser";
import type { FsEvent, FsRequest, FsResponse } from "./fs";
import type { GitRequest, GitResponse, GitResponseFor } from "./git";
import type { InfoEvent, InfoRequest, InfoResponse } from "./info";
import type { LspEvent, LspRequest, LspResponse } from "./lsp";
import type { NetproxyEvent, NetproxyRequest, NetproxyResponse } from "./netproxy";
import type { OpenRequest } from "./open";
import type { OrchestrationEvent, OrchestrationRequest, OrchestrationResponse } from "./orchestration";
import type { PermissionsEvent, PermissionsRequest, PermissionsResponse } from "./permissions";
import type { PolicyEvent, PolicyRequest, PolicyResponse } from "./policy";
import type { PrEvent, PrRequest, PrResponse, PrResponseFor } from "./pr";
import type { ProjectsEvent, ProjectsRequest, ProjectsResponse } from "./projects";
import type { ResourcesEvent } from "./resources";
import type { ReviewEvent, ReviewRequest, ReviewResponse } from "./review";
import type { SearchEvent, SearchRequest, SearchResponse } from "./search";
import type { SecretsEvent, SecretsRequest, SecretsResponse } from "./secrets";
import type { ServersEvent, ServersRequest, ServersResponse, ServersResponseFor } from "./servers";
import type { TerminalsEvent, TerminalsRequest, TerminalsResponse } from "./terminals";

export type * from "./agents";
export type * from "./audit";
export type * from "./browser";
export type * from "./fs";
export type * from "./git";
// `review.ts` has its own `GitChange` (a review item's changed git file); the
// index exports the git panel's.
export type { GitChange } from "./git";
export type * from "./info";
export type * from "./lsp";
// `servers.ts` and `lsp.ts` both have `ServerId` and `ServerInfo`; the index
// exports the language servers' and names the managed servers' apart.
export type { ServerId, ServerInfo } from "./lsp";
export type * from "./netproxy";
export type * from "./open";
export type * from "./orchestration";
export type * from "./permissions";
export type * from "./policy";
export type * from "./pr";
export type * from "./projects";
export type * from "./resources";
export type * from "./review";
export type * from "./search";
export type * from "./secrets";
export type * from "./servers";
export type { ServerId as DevServerId, ServerInfo as DevServerInfo } from "./servers";
export type * from "./terminals";

export type Request =
  | InfoRequest
  | ProjectsRequest
  | FsRequest
  | OpenRequest
  | TerminalsRequest
  | AgentsRequest
  | PermissionsRequest
  | ReviewRequest
  | OrchestrationRequest
  | SearchRequest
  | PolicyRequest
  | SecretsRequest
  | AuditRequest
  | NetproxyRequest
  | GitRequest
  | LspRequest
  | ServersRequest
  | BrowserRequest
  | PrRequest;

export type Response =
  | InfoResponse
  | ProjectsResponse
  | FsResponse
  | TerminalsResponse
  | AgentsResponse
  | PermissionsResponse
  | ReviewResponse
  | OrchestrationResponse
  | SearchResponse
  | PolicyResponse
  | SecretsResponse
  | AuditResponse
  | NetproxyResponse
  | GitResponse
  | LspResponse
  | ServersResponse
  | BrowserResponse
  | PrResponse
  | { type: "ok" };

export type CoreEvent =
  | InfoEvent
  | ProjectsEvent
  | FsEvent
  | TerminalsEvent
  | AgentsEvent
  | PermissionsEvent
  | ReviewEvent
  | OrchestrationEvent
  | SearchEvent
  | PolicyEvent
  | SecretsEvent
  | AuditLogEvent
  | NetproxyEvent
  | LspEvent
  | ServersEvent
  | BrowserEvent
  | PrEvent
  | ResourcesEvent;

export type ErrorCode = "bad_request" | "not_found" | "conflict" | "forbidden" | "unavailable" | "internal";

export type ProtocolError = { code: ErrorCode; message: string };

export type ServerFrame =
  | { id: number; result: Response }
  | { id: number | null; error: ProtocolError }
  | { event: CoreEvent };

/** The response type a given request method resolves to. */
export type ResponseFor<R extends Request> = R extends { method: "info.get" }
  ? Extract<Response, { type: "info" }>
  : R extends { method: "projects.list" | "projects.remove" }
    ? Extract<Response, { type: "projects.list" }>
    : R extends { method: "projects.add" | "projects.open" | "projects.trust" }
      ? Extract<Response, { type: "projects.project" }>
      : R extends { method: "projects.get_state" }
        ? Extract<Response, { type: "projects.state" }>
        : R extends { method: "projects.set_state" }
          ? { type: "ok" }
          : R extends { method: "fs.list_dir" }
            ? Extract<Response, { type: "fs.dir" }>
            : R extends { method: "fs.read_file" }
              ? Extract<Response, { type: "fs.file" }>
              : R extends { method: "fs.write_file" }
                ? Extract<Response, { type: "fs.written" }>
                : R extends { method: "fs.list_files" }
                  ? Extract<Response, { type: "fs.files" }>
                  : R extends {
                        method:
                          | "open.external"
                          | "open.url"
                          | "open.reveal"
                          | "fs.create"
                          | "fs.rename"
                          | "fs.copy"
                          | "fs.trash";
                      }
                    ? { type: "ok" }
                    : R extends { method: "terminals.open" }
                      ? Extract<Response, { type: "terminals.opened" }>
                      : R extends { method: "terminals.scrollback" }
                        ? Extract<Response, { type: "terminals.scrollback" }>
                        : R extends { method: "agents.list" }
                          ? Extract<Response, { type: "agents.list" }>
                          : R extends { method: "agents.start" | "agents.resume" }
                            ? Extract<Response, { type: "agents.session" }>
                            : R extends { method: "agents.transcript" }
                              ? Extract<Response, { type: "agents.transcript" }>
                              : R extends { method: "agents.setup" | "agents.set_path" }
                                ? Extract<Response, { type: "agents.setup" }>
                                : R extends {
                                      method: "agents.accounts" | "agents.save_account" | "agents.remove_account";
                                    }
                                  ? Extract<Response, { type: "agents.accounts" }>
                                  : R extends { method: "agents.project_config" }
                                    ? Extract<Response, { type: "agents.project_config" }>
                                    : R extends { method: "agents.capacity" }
                                      ? Extract<Response, { type: "agents.capacity" }>
                                      : R extends { method: "agents.limit" | "agents.set_limit" }
                                        ? Extract<Response, { type: "agents.limit" }>
                                        : R extends { method: "agents.research_limit" | "agents.set_research_limit" }
                                          ? Extract<Response, { type: "agents.research_limit" }>
                                          : R extends { method: "permissions.pending" }
                                            ? Extract<Response, { type: "permissions.pending" }>
                                            : R extends {
                                                  method:
                                                    | "terminals.input"
                                                    | "terminals.resize"
                                                    | "terminals.close"
                                                    | "agents.send"
                                                    | "agents.interrupt"
                                                    | "agents.stop"
                                                    | "agents.set_project_account"
                                                    | "permissions.answer"
                                                    | "review.comment"
                                                    | "orchestration.stop"
                                                    | "orchestration.retry_setup"
                                                    | "orchestration.remove_leftover"
                                                    | "search.cancel";
                                                }
                                              ? { type: "ok" }
                                              : PhaseThreeResponseFor<R>;

/** `ResponseFor` for the review and orchestration domains. */
type PhaseThreeResponseFor<R extends Request> = R extends { method: "review.list" }
  ? Extract<Response, { type: "review.list" }>
  : R extends { method: "review.diff" }
    ? Extract<Response, { type: "review.diff" }>
    : R extends {
          method:
            | "review.approve"
            | "review.reject"
            | "review.undo_reject"
            | "review.resolve"
            | "review.take"
            | "review.undo_take";
        }
      ? Extract<Response, { type: "review.done" }>
      : R extends { method: "orchestration.start" }
        ? Extract<Response, { type: "orchestration.task" }>
        : R extends { method: "orchestration.list" }
          ? Extract<Response, { type: "orchestration.list" }>
          : R extends { method: "orchestration.leftovers" }
            ? Extract<Response, { type: "orchestration.leftovers" }>
            : R extends { method: "search.start" }
              ? Extract<Response, { type: "search.started" }>
              : R extends { method: "policy.get" | "policy.set" }
                ? Extract<Response, { type: "policy.policy" }>
                : R extends { method: "secrets.list" }
                  ? Extract<Response, { type: "secrets.list" }>
                  : R extends { method: "secrets.set" | "secrets.remove" }
                    ? { type: "ok" }
                    : R extends { method: "audit.list" }
                      ? Extract<Response, { type: "audit.entries" }>
                      : R extends { method: "netproxy.get_allowlist" | "netproxy.set_allowlist" }
                        ? Extract<Response, { type: "netproxy.allowlist" }>
                        : R extends GitRequest
                          ? GitResponseFor<R["method"]>
                          : R extends { method: "lsp.status" }
                            ? Extract<Response, { type: "lsp.status" }>
                            : R extends { method: "lsp.start" }
                              ? Extract<Response, { type: "lsp.server" }>
                              : R extends { method: "lsp.send" | "lsp.stop" }
                                ? { type: "ok" }
                                : R extends ServersRequest
                                  ? ServersResponseFor<R["method"]>
                                  : R extends BrowserRequest
                                    ? BrowserResponseFor<R["method"]>
                                    : R extends { method: CheckpointMethod }
                                      ? CheckpointResponseFor<R["method"]>
                                      : R extends PrRequest
                                        ? PrResponseFor<R["method"]>
                                        : never;
