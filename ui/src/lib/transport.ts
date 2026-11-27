// One transport interface for every front end (ADR 0002): Tauri IPC inside
// the desktop window, WebSocket in headless mode. Both carry the same frames.

import type { CoreEvent, ProjectId, ProtocolError, Request, Response, ResponseFor, ServerFrame } from "./protocol";

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
  /** What the person drops onto the window. Absent where a page cannot see dropped paths. */
  onDrop?(listener: (drop: WindowDrop) => void): () => void;
  /** Where files dragged in from outside the window are, while they are over it; null once they leave or drop. */
  onDragOver?(listener: (at: Point | null) => void): () => void;
  /**
   * Copies the drop `id` into the folder `dir` of `project`. With `"ask"`, a
   * name that exists already copies nothing and comes back in `names`; the
   * drop then waits for the person's answer. Present with `onDrop`.
   */
  importDrop?(id: string, project: ProjectId, dir: string, clash: DropClash): Promise<DropImport>;
  /** Ids of the commands the person picks in the window's menu bar. Absent where there is none. */
  onMenu?(listener: (command: string) => void): () => void;
  /** The remote machine this window shows (ADR 0016); absent in the laptop's own window. */
  remote?: RemoteWindow;
  /** Opens a remote machine in a window of its own, or focuses it. Absent in a remote window. */
  openRemote?(machine: RemoteMachine): Promise<void>;
}

/** A remote machine as its window knows it: from the app, never from the machine. */
type RemoteMachine = { id: string; name: string };

/** The link to a remote machine; `reason` and `details` come with `closed` and `failed`. */
export type RemoteLink = {
  state: "connecting" | "open" | "closed" | "failed";
  reason: string | null;
  /** What `ssh` said: the remote machine's own text, to show as plain text only. */
  details: string | null;
};

export interface RemoteWindow {
  machine: RemoteMachine;
  /** Reports the link's state at once, then every change. */
  onLink(listener: (link: RemoteLink) => void): () => void;
  reconnect(): void;
  disconnect(): void;
}

/** Keeps the last link state and tells listeners. */
class LinkState {
  private link: RemoteLink = { state: "connecting", reason: null, details: null };
  private listeners = new Set<(link: RemoteLink) => void>();

  set(link: RemoteLink): void {
    this.link = link;
    for (const listener of this.listeners) listener(link);
  }

  on(listener: (link: RemoteLink) => void): () => void {
    this.listeners.add(listener);
    listener(this.link);
    return () => this.listeners.delete(listener);
  }
}

/** A point in the page, in CSS pixels. */
export type Point = { x: number; y: number };

/** A drop onto the window: the dropped paths, where it landed, and the id `importDrop` copies it by. */
export type WindowDrop = Point & { id: string; paths: string[] };

/** What to do with a dropped name that exists already. */
export type DropClash = "ask" | "replace" | "keep_both";

export type DropImport =
  | { kind: "clashes"; names: string[] }
  | { kind: "copied"; paths: string[]; skipped_links: string[] };

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

/** What the app sends on a window's channel (`app/src/mailbox.rs`). */
type AppMessage =
  | { frame: string }
  | { menu: string }
  | { drop: WindowDrop }
  | { dragOver: Point | null }
  | { sshStatus: RemoteLink & { id: string } };

/** The machine the app said this window shows (`app/src/remote.rs`). */
function appRemote(): RemoteMachine | null {
  const given = (window as { __OVRA_REMOTE__?: RemoteMachine }).__OVRA_REMOTE__;
  if (given && typeof given.id === "string" && typeof given.name === "string") return given;
  // A machine's window without its identity must not pass for the laptop's.
  const label = (window as { __TAURI_INTERNALS__?: { metadata?: { currentWindow?: { label?: unknown } } } })
    .__TAURI_INTERNALS__?.metadata?.currentWindow?.label;
  if (typeof label !== "string" || label.startsWith("ssh-")) {
    throw new Error("This window does not say which machine it shows.");
  }
  return null;
}

export function connectTauri(): Transport {
  let router: FrameRouter;
  const machine = appRemote();
  const link = new LinkState();
  const menuListeners = new Set<(command: string) => void>();
  const dropListeners = new Set<(drop: WindowDrop) => void>();
  const overListeners = new Set<(at: Point | null) => void>();
  // Everything the app tells this window comes on the window's own channel:
  // app events would reach every window, a remote machine's too.
  const ready = import("@tauri-apps/api/core").then(async (core) => {
    const channel = new core.Channel<AppMessage>((message) => {
      if ("frame" in message) router.deliver(message.frame);
      else if ("menu" in message) for (const listener of menuListeners) listener(message.menu);
      else if ("drop" in message) for (const listener of dropListeners) listener(message.drop);
      // The app follows the drag too, so its position is in CSS pixels on every platform.
      else if ("dragOver" in message) for (const listener of overListeners) listener(message.dragOver);
      else if ("sshStatus" in message && machine && message.sshStatus.id === machine.id) {
        const { state, reason, details } = message.sshStatus;
        link.set({ state, reason, details });
      }
    });
    await core.invoke("ovra_subscribe", { channel });
    return core.invoke;
  });
  router = new FrameRouter((frame) => {
    ready
      .then((invoke) => invoke<string>("ovra_call", { frame }))
      .then((reply) => router.deliver(reply))
      .catch((err: unknown) => router.failAll(err instanceof Error ? err : new Error(String(err))));
  });
  const menu = {
    onMenu: (listener: (command: string) => void) => {
      menuListeners.add(listener);
      return () => menuListeners.delete(listener);
    },
  };
  if (machine) {
    // Links open in this computer's browser through the app, which refuses
    // any that point at this computer: the machine's core cannot open them.
    const call = router.call.bind(router);
    router.call = (<R extends Request>(request: R) =>
      request.method === "open.url"
        ? ready
            .then((invoke) => invoke("ovra_open_url", { url: request.params.url }))
            .then(
              () => ({ type: "ok" }),
              (err: unknown) => {
                throw err instanceof Error ? err : new Error(String(err));
              },
            )
        : call(request)) as typeof router.call;
    const remote: RemoteWindow = {
      machine,
      onLink: (listener) => link.on(listener),
      reconnect: () => void ready.then((invoke) => invoke("ovra_remote_reconnect")),
      disconnect: () => void ready.then((invoke) => invoke("ovra_remote_disconnect")),
    };
    return Object.assign(router, menu, { remote });
  }
  return Object.assign(router, menu, {
    pickFolder: () => ready.then((invoke) => invoke<string | null>("ovra_pick_folder")),
    onDrop: (listener: (drop: WindowDrop) => void) => {
      dropListeners.add(listener);
      return () => dropListeners.delete(listener);
    },
    onDragOver: (listener: (at: Point | null) => void) => {
      overListeners.add(listener);
      return () => overListeners.delete(listener);
    },
    importDrop: (id: string, project: ProjectId, dir: string, clash: DropClash) =>
      ready
        .then((invoke) => invoke<DropImport>("ovra_import_drop", { dropId: id, project, targetDir: dir, clash }))
        .catch((err: unknown) => {
          throw err instanceof Error ? err : new Error(String(err));
        }),
    openRemote: (remote: RemoteMachine) => ready.then((invoke) => invoke<void>("ovra_remote_show", { id: remote.id })),
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
  const query = new URLSearchParams(location.search);
  const token = query.get("token") ?? "";
  const scheme = location.protocol === "https:" ? "wss" : "ws";
  const serverUp = () =>
    fetch(`${location.origin}/`, { method: "HEAD", cache: "no-store" }).then(
      () => true,
      () => false,
    );
  // e2e only: a daemon with `--fake-ssh` bridges `/ws/ssh/<id>` to a fake
  // remote core. A failure closes the socket with its reason.
  const id = query.get("remote");
  if (id) {
    const machine = { id, name: query.get("name") ?? id };
    const link = new LinkState();
    const url = `${scheme}://${location.host}/ws/ssh/${encodeURIComponent(id)}?token=${encodeURIComponent(token)}`;
    const openSocket = () => {
      const socket = new WebSocket(url);
      // Open once the machine answers: the bridge accepts the socket before
      // it knows whether the machine's core starts.
      socket.addEventListener("message", () => link.set({ state: "open", reason: null, details: null }), {
        once: true,
      });
      socket.addEventListener("close", (e) =>
        link.set({ state: "failed", reason: e.reason || "exited", details: null }),
      );
      return socket;
    };
    const remote: RemoteWindow = { machine, onLink: (listener) => link.on(listener), reconnect() {}, disconnect() {} };
    return Object.assign(connectSocket(openSocket, serverUp), { remote });
  }
  const url = `${scheme}://${location.host}/ws?token=${encodeURIComponent(token)}`;
  return Object.assign(
    connectSocket(() => new WebSocket(url), serverUp),
    {
      openRemote: async (machine: RemoteMachine) => {
        const page = `/?token=${encodeURIComponent(token)}&remote=${encodeURIComponent(machine.id)}&name=${encodeURIComponent(machine.name)}`;
        window.open(page, `ssh-${machine.id}`);
      },
    },
  );
}

export function connect(): Transport {
  return inTauri() ? connectTauri() : connectWebSocket();
}
