// Mirrors crates/core/src/protocol/. One file per domain, like the Rust side.

import type { AgentsEvent, AgentsRequest, AgentsResponse } from "./agents";
import type { FsEvent, FsRequest, FsResponse } from "./fs";
import type { InfoEvent, InfoRequest, InfoResponse } from "./info";
import type { OpenRequest } from "./open";
import type { OrchestrationEvent, OrchestrationRequest, OrchestrationResponse } from "./orchestration";
import type { PermissionsEvent, PermissionsRequest, PermissionsResponse } from "./permissions";
import type { ProjectsEvent, ProjectsRequest, ProjectsResponse } from "./projects";
import type { ReviewEvent, ReviewRequest, ReviewResponse } from "./review";
import type { TerminalsEvent, TerminalsRequest, TerminalsResponse } from "./terminals";

export type * from "./agents";
export type * from "./fs";
export type * from "./info";
export type * from "./open";
export type * from "./orchestration";
export type * from "./permissions";
export type * from "./projects";
export type * from "./review";
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
  | OrchestrationRequest;

export type Response =
  | InfoResponse
  | ProjectsResponse
  | FsResponse
  | TerminalsResponse
  | AgentsResponse
  | PermissionsResponse
  | ReviewResponse
  | OrchestrationResponse
  | { type: "ok" };

export type CoreEvent =
  | InfoEvent
  | ProjectsEvent
  | FsEvent
  | TerminalsEvent
  | AgentsEvent
  | PermissionsEvent
  | ReviewEvent
  | OrchestrationEvent;

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
                  : R extends { method: "open.external" | "open.url" }
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
                                : R extends { method: "agents.project_config" }
                                  ? Extract<Response, { type: "agents.project_config" }>
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
                                            | "permissions.answer"
                                            | "review.comment"
                                            | "orchestration.stop";
                                        }
                                      ? { type: "ok" }
                                      : PhaseThreeResponseFor<R>;

/** `ResponseFor` for the review and orchestration domains. */
type PhaseThreeResponseFor<R extends Request> = R extends { method: "review.list" }
  ? Extract<Response, { type: "review.list" }>
  : R extends { method: "review.diff" }
    ? Extract<Response, { type: "review.diff" }>
    : R extends { method: "review.approve" | "review.reject" | "review.undo_reject" }
      ? Extract<Response, { type: "review.done" }>
      : R extends { method: "orchestration.start" }
        ? Extract<Response, { type: "orchestration.task" }>
        : R extends { method: "orchestration.list" }
          ? Extract<Response, { type: "orchestration.list" }>
          : never;
