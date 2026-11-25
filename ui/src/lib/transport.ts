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

/**
 * Whether frames can reach the core right now. `refused`: the server is up
 * but no longer accepts this page's token (the core restarted); it is final.
 */
export type ConnectionStatus = "connecting" | "open" | "closed" | "refused";

export interface Transport {
  call<R extends Request>(request: R): Promise<ResponseFor<R>>;
  onEvent(listener: (event: CoreEvent) => void): () => void;
  /** Reports the current status at once, then every change. Absent means always open. */
  onStatus?(listener: (status: ConnectionStatus) => void): () => void;
  /**
   * Asks the person for a folder with the system's dialog and resolves to its
   * path, or null when they cancel. Absent where there is no such dialog
   * (headless mode in a browser).
   */
  pickFolder?(): Promise<string | null>;
  /** Paths of what the person drops onto the window. Absent where a page cannot see dropped paths. */
  onDrop?(listener: (paths: string[]) => void): () => void;
}

/** Sends a serialized frame; replies and events arrive via `deliver`. */
type Wire = (frame: string) => void;

const CLOSED_MESSAGE = "connection to core closed";

/** Matches replies to calls and fans events out. Transport-agnostic. */
export class FrameRouter implements Transport {
  private nextId = 1;
  private pending = new Map<number, { resolve: (r: Response) => void; reject: (e: Error) => void }>();
  private listeners = new Set<(event: CoreEvent) => void>();
  private statusListeners = new Set<(status: ConnectionStatus) => void>();

  constructor(
    private readonly send: Wire,
    private status: ConnectionStatus = "open",
  ) {}

  call<R extends Request>(request: R): Promise<ResponseFor<R>> {
    if (this.status === "closed" || this.status === "refused") return Promise.reject(new Error(CLOSED_MESSAGE));
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve: resolve as (r: Response) => void, reject });
      try {
        this.send(JSON.stringify({ id, ...request }));
      } catch (err) {
        this.pending.delete(id);
        reject(err instanceof Error ? err : new Error(String(err)));
      }
    });
  }

  onEvent(listener: (event: CoreEvent) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  onStatus(listener: (status: ConnectionStatus) => void): () => void {
    this.statusListeners.add(listener);
    listener(this.status);
    return () => this.statusListeners.delete(listener);
  }

  /** Hands `event` to the listeners as if the core had sent it. */
  emit(event: CoreEvent): void {
    for (const listener of this.listeners) listener(event);
  }

  /** Moves to `status`; closing rejects every pending call. */
  setStatus(status: ConnectionStatus): void {
    if (status === "closed" || status === "refused") this.failAll(new Error(CLOSED_MESSAGE));
    if (status === this.status) return;
    this.status = status;
    for (const listener of this.statusListeners) listener(status);
  }

  deliver(raw: string): void {
    const frame = JSON.parse(raw) as ServerFrame;
    if ("event" in frame) {
      this.emit(frame.event);
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

export function connectTauri(): Transport {
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
  const dropListeners = new Set<(paths: string[]) => void>();
  import("@tauri-apps/api/webview")
    .then(({ getCurrentWebview }) =>
      getCurrentWebview().onDragDropEvent((e) => {
        if (e.payload.type !== "drop") return;
        for (const listener of dropListeners) listener(e.payload.paths);
      }),
    )
    .catch((err: unknown) => console.error("dropping folders onto the window is unavailable", err));
  return Object.assign(router, {
    pickFolder: () => ready.then((invoke) => invoke<string | null>("ovra_pick_folder")),
    onDrop: (listener: (paths: string[]) => void) => {
      dropListeners.add(listener);
      return () => dropListeners.delete(listener);
    },
  });
}

/** The part of `WebSocket` the transport uses, so tests can pass a fake. */
export interface SocketLike {
  send(data: string): void;
  addEventListener(type: "open" | "close", listener: () => void): void;
  addEventListener(type: "message", listener: (event: { data: unknown }) => void): void;
}

const RECONNECT_INITIAL_MS = 250;
const RECONNECT_MAX_MS = 10_000;
/** Failed opens in a row before asking whether the server is refusing us. */
const REFUSED_AFTER = 2;

/**
 * Runs the router over sockets from `openSocket`. Frames wait in a queue only
 * while a socket is connecting; after a close, calls fail at once and a new
 * socket is opened after a delay that doubles up to `RECONNECT_MAX_MS` and
 * resets after a successful open.
 *
 * After a reconnect every listener gets an `info.resync`, since events sent
 * while the socket was down are lost. When sockets keep failing to open but
 * `serverUp` says the server answers, the server is refusing the token (the
 * core restarted with a new one): the status becomes `refused` and retrying
 * stops.
 */
export function connectSocket(openSocket: () => SocketLike, serverUp: () => Promise<boolean>): FrameRouter {
  let socket: SocketLike;
  let queue: string[] = [];
  let delay = RECONNECT_INITIAL_MS;
  let status: ConnectionStatus = "connecting";
  let everOpen = false;
  let failedOpens = 0;
  const router = new FrameRouter((frame) => {
    if (status === "open") socket.send(frame);
    else queue.push(frame);
  }, status);
  const setStatus = (next: ConnectionStatus) => {
    status = next;
    router.setStatus(next);
  };

  const retry = () => {
    setTimeout(open, delay);
    delay = Math.min(delay * 2, RECONNECT_MAX_MS);
  };

  const open = () => {
    setStatus("connecting");
    let opened = false;
    socket = openSocket();
    socket.addEventListener("open", () => {
      opened = true;
      failedOpens = 0;
      delay = RECONNECT_INITIAL_MS;
      setStatus("open");
      for (const frame of queue.splice(0)) socket.send(frame);
      if (everOpen) router.emit({ type: "info.resync", data: { skipped: 0 } });
      everOpen = true;
    });
    socket.addEventListener("message", (e) => router.deliver(String(e.data)));
    socket.addEventListener("close", () => {
      queue = [];
      setStatus("closed");
      if (opened || ++failedOpens < REFUSED_AFTER) {
        retry();
        return;
      }
      void serverUp().then((up) => {
        if (up) setStatus("refused");
        else retry();
      });
    });
  };
  open();
  return router;
}

function connectWebSocket(): Transport {
  const token = new URLSearchParams(location.search).get("token") ?? "";
  const scheme = location.protocol === "https:" ? "wss" : "ws";
  const url = `${scheme}://${location.host}/ws?token=${encodeURIComponent(token)}`;
  const serverUp = () =>
    fetch(`${location.origin}/`, { method: "HEAD", cache: "no-store" }).then(
      () => true,
      () => false,
    );
  return connectSocket(() => new WebSocket(url), serverUp);
}

export function connect(): Transport {
  return inTauri() ? connectTauri() : connectWebSocket();
}
