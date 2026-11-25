// Mirrors crates/core/src/protocol/lsp.rs. Language servers for trusted
// projects (ADR 0007, docs/specs/lsp.md).

import type { ProjectId } from "./projects";

export type ServerId = string;

export type ServerState =
  | { state: "starting" }
  | { state: "running" }
  | { state: "stopped" }
  | { state: "failed"; reason: string };

/** One language server of a project. `sandbox` is null when it runs unsandboxed. */
export type ServerInfo = {
  id: ServerId;
  project: ProjectId;
  /** The editor's language id: `rust`, `typescript`, ... */
  language: string;
  /** The server's name, such as `rust-analyzer`. */
  name: string;
  sandbox: string | null;
} & ServerState;

/** A server Ovra knows for a language; `path` is null when it is not installed. */
export type KnownServer = { language: string; name: string; path: string | null; install: string };

export type LspStatus = {
  /** Servers start only for a trusted project. */
  trusted: boolean;
  /** The sandbox servers would run in; null for none. */
  sandbox: string | null;
  servers: ServerInfo[];
  known: KnownServer[];
};

/** A JSON-RPC 2.0 message, with `ovra:///<path>` URIs on the UI's side. */
export type JsonRpcMessage = { jsonrpc: "2.0"; [key: string]: unknown };

export type LspRequest =
  | { method: "lsp.status"; params: { project: ProjectId } }
  /** `forbidden` for an untrusted project; `not_found` when no server is installed for the language. */
  | { method: "lsp.start"; params: { project: ProjectId; language: string } }
  /** `bad_request` for a message naming a `file:` URI or a path outside the project. */
  | { method: "lsp.send"; params: { server: ServerId; message: JsonRpcMessage } }
  | { method: "lsp.stop"; params: { server: ServerId } };

export type LspResponse = { type: "lsp.status"; data: LspStatus } | { type: "lsp.server"; data: ServerInfo };

export type LspEvent =
  /** One message from the server, after the core's filter. */
  | { type: "lsp.message"; data: { server: ServerId; message: JsonRpcMessage } }
  | { type: "lsp.changed"; data: { server: ServerInfo } };
