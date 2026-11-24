// Mirrors crates/core/src/protocol/. One file per domain, like the Rust side.

import type { FsEvent, FsRequest, FsResponse } from "./fs";
import type { InfoEvent, InfoRequest, InfoResponse } from "./info";
import type { OpenRequest } from "./open";
import type { ProjectsEvent, ProjectsRequest, ProjectsResponse } from "./projects";

export type * from "./fs";
export type * from "./info";
export type * from "./open";
export type * from "./projects";

export type Request = InfoRequest | ProjectsRequest | FsRequest | OpenRequest;

export type Response = InfoResponse | ProjectsResponse | FsResponse | { type: "ok" };

export type CoreEvent = InfoEvent | ProjectsEvent | FsEvent;

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
    : R extends { method: "projects.add" | "projects.open" }
      ? Extract<Response, { type: "projects.project" }>
      : R extends { method: "fs.list_dir" }
        ? Extract<Response, { type: "fs.dir" }>
        : R extends { method: "fs.read_file" }
          ? Extract<Response, { type: "fs.file" }>
          : R extends { method: "fs.write_file" }
            ? Extract<Response, { type: "fs.written" }>
            : R extends { method: "open.external" | "open.url" }
              ? { type: "ok" }
              : never;
