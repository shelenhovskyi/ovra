// One transport interface for every front end (ADR 0002): Tauri IPC inside
// the desktop window, WebSocket in headless mode. Both carry the same frames.

import type { CoreEvent, ProtocolError, Request, Response, ResponseFor, ServerFrame } from "./protocol";

/** A failed call; `code` is the core's error code. */
export class CoreError extends Error {
  constructor(readonly error: ProtocolError) {
    super(`${error.code}: ${error.message}`);
  }
  get code() {
    return this.error.code;
  }
}

export interface Transport {
  call<R extends Request>(request: R): Promise<ResponseFor<R>>;
  onEvent(listener: (event: CoreEvent) => void): () => void;
}

/** Sends a serialized frame; replies and events arrive via `deliver`. */
type Wire = (frame: string) => void;

/** Matches replies to calls and fans events out. Transport-agnostic. */
export class FrameRouter implements Transport {
  private nextId = 1;
  private pending = new Map<number, { resolve: (r: Response) => void; reject: (e: Error) => void }>();
  private listeners = new Set<(event: CoreEvent) => void>();

  constructor(private readonly send: Wire) {}

  call<R extends Request>(request: R): Promise<ResponseFor<R>> {
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve: resolve as (r: Response) => void, reject });
      this.send(JSON.stringify({ id, ...request }));
    });
  }

  onEvent(listener: (event: CoreEvent) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  deliver(raw: string): void {
    const frame = JSON.parse(raw) as ServerFrame;
    if ("event" in frame) {
      for (const listener of this.listeners) listener(frame.event);
      return;
    }
    if (frame.id === null) return;
    const waiter = this.pending.get(frame.id);
    if (!waiter) return;
    this.pending.delete(frame.id);
    if ("result" in frame) waiter.resolve(frame.result);
    else waiter.reject(new CoreError(frame.error));
  }

  failAll(error: Error): void {
    for (const waiter of this.pending.values()) waiter.reject(error);
    this.pending.clear();
  }
}

function inTauri(): boolean {
  return "__TAURI_INTERNALS__" in window;
}

function connectTauri(): Transport {
  let router: FrameRouter;
  const ready = Promise.all([import("@tauri-apps/api/core"), import("@tauri-apps/api/event")]).then(([core, event]) => {
    void event.listen<string>("ovra://frame", (e) => router.deliver(e.payload));
    return core.invoke;
  });
  router = new FrameRouter((frame) => {
    ready
      .then((invoke) => invoke<string>("ovra_call", { frame }))
      .then((reply) => router.deliver(reply))
      .catch((err: unknown) => router.failAll(err instanceof Error ? err : new Error(String(err))));
  });
  return router;
}

function connectWebSocket(): Transport {
  const token = new URLSearchParams(location.search).get("token") ?? "";
  const scheme = location.protocol === "https:" ? "wss" : "ws";
  const socket = new WebSocket(`${scheme}://${location.host}/ws?token=${encodeURIComponent(token)}`);
  const queue: string[] = [];
  const router = new FrameRouter((frame) => {
    if (socket.readyState === WebSocket.OPEN) socket.send(frame);
    else queue.push(frame);
  });
  socket.addEventListener("open", () => {
    for (const frame of queue.splice(0)) socket.send(frame);
  });
  socket.addEventListener("message", (e) => router.deliver(String(e.data)));
  socket.addEventListener("close", () => router.failAll(new Error("connection to core closed")));
  return router;
}

export function connect(): Transport {
  return inTauri() ? connectTauri() : connectWebSocket();
}
