// Mirrors crates/core/src/protocol/servers.rs: dev servers in sandbox pods.

import type { SessionId } from "./agents";
import type { ProjectId } from "./projects";

export type ServerId = string;

/** One server as the person defines it in `.ovra/servers.json`. */
export type ServerDef = {
  name: string;
  /** A program and its arguments, run without a shell. */
  command: string[];
  /** Relative to the work dir; absent is the work dir itself. */
  cwd?: string;
  env?: Record<string, string>;
  port: number;
  /** Absent: ready when the port accepts a connection. */
  ready?: ServerReady;
};

export type ServerReady = { log?: string; http?: string };

export type ServerStatus =
  | { state: "starting" }
  | { state: "ready" }
  | { state: "stopping" }
  /** `code` is null when it was killed by a signal. */
  | { state: "exited"; code: number | null }
  | { state: "failed"; reason: string };

export type StartedBy = { kind: "person" } | { kind: "agent"; session: SessionId };

export type ServerInfo = {
  id: ServerId;
  project: ProjectId;
  name: string;
  /** The session whose worktree it runs in; null for the main checkout. */
  work_dir: SessionId | null;
  status: ServerStatus;
  generation: number;
  port: number;
  started_by: StartedBy;
};

export type ServerList = {
  definitions: ServerDef[];
  servers: ServerInfo[];
  /** Why `.ovra/servers.json` could not be read. */
  error: string | null;
};

/** `n` counts a server's lines from 1; `text` keeps ANSI escapes. */
export type LogLine = { n: number; text: string };

export type LogPage = { lines: LogLine[]; next: number };

export type ServersRequest =
  | { method: "servers.list"; params: { project: ProjectId; work_dir?: SessionId | null } }
  | { method: "servers.start"; params: { project: ProjectId; name: string; work_dir?: SessionId | null } }
  /** SIGTERM, then SIGKILL after `grace_ms` (default 5000). */
  | { method: "servers.stop"; params: { id: ServerId; grace_ms?: number | null } }
  | { method: "servers.restart"; params: { id: ServerId } }
  | {
      method: "servers.logs";
      params: { id: ServerId; since?: number | null; limit?: number | null; grep?: string | null };
    }
  /** Makes a ready server reachable on the host for the person's browser. */
  | { method: "servers.open"; params: { id: ServerId } };

export type ServersResponse =
  | { type: "servers.list"; data: ServerList }
  | { type: "servers.server"; data: ServerInfo }
  | { type: "servers.logs"; data: LogPage }
  | { type: "servers.opened"; data: { url: string } };

export type ServersEvent =
  | {
      type: "servers.status";
      data: { id: ServerId; project: ProjectId; status: ServerStatus; generation: number };
    }
  | { type: "servers.output"; data: { id: ServerId; lines: LogLine[] } };

/** The response type of each `servers.*` method. */
export type ServersResponseFor<M extends ServersRequest["method"]> = M extends "servers.list"
  ? Extract<ServersResponse, { type: "servers.list" }>
  : M extends "servers.start" | "servers.restart"
    ? Extract<ServersResponse, { type: "servers.server" }>
    : M extends "servers.logs"
      ? Extract<ServersResponse, { type: "servers.logs" }>
      : M extends "servers.open"
        ? Extract<ServersResponse, { type: "servers.opened" }>
        : { type: "ok" };
