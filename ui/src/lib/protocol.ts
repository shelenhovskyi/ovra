// Mirrors crates/core/src/protocol.rs. Keep the two in sync.

export type Request = { method: "get_info"; params?: undefined };

export type CoreInfo = { name: string; version: string; mode: string };

export type Response = { type: "info"; data: CoreInfo };

export type CoreEvent = { type: "ready" };

type ProtocolError = { code: "bad_request"; message: string };

export type ServerFrame =
  | { id: number; result: Response }
  | { id: number | null; error: ProtocolError }
  | { event: CoreEvent };
