import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { type ConnectionStatus, CoreError, connectSocket, FrameRouter, type SocketLike } from "./transport";

describe("FrameRouter", () => {
  it("matches replies to calls by id", async () => {
    const sent: string[] = [];
    const router = new FrameRouter((frame) => sent.push(frame));
    const reply = router.call({ method: "info.get" });
    expect(JSON.parse(sent[0] ?? "")).toEqual({ id: 1, method: "info.get" });
    router.deliver(
      JSON.stringify({ id: 1, result: { type: "info", data: { name: "ovra", version: "0.1.0", mode: "test" } } }),
    );
    await expect(reply).resolves.toEqual({ type: "info", data: { name: "ovra", version: "0.1.0", mode: "test" } });
  });

  it("rejects on error frames", async () => {
    const router = new FrameRouter(() => {});
    const reply = router.call({ method: "info.get" });
    router.deliver(JSON.stringify({ id: 1, error: { code: "bad_request", message: "nope" } }));
    await expect(reply).rejects.toThrow("bad_request: nope");
    await expect(reply).rejects.toBeInstanceOf(CoreError);
  });

  it("delivers events to listeners until unsubscribed", () => {
    const router = new FrameRouter(() => {});
    const seen: string[] = [];
    const off = router.onEvent((event) => seen.push(event.type));
    router.deliver(JSON.stringify({ event: { type: "info.ready" } }));
    off();
    router.deliver(JSON.stringify({ event: { type: "info.ready" } }));
    expect(seen).toEqual(["info.ready"]);
  });

  it("fails pending calls when the connection drops", async () => {
    const router = new FrameRouter(() => {});
    const reply = router.call({ method: "info.get" });
    router.failAll(new Error("closed"));
    await expect(reply).rejects.toThrow("closed");
  });

  it("rejects a call at once while closed and when the wire throws", async () => {
    const router = new FrameRouter(() => {
      throw new Error("wire down");
    });
    await expect(router.call({ method: "info.get" })).rejects.toThrow("wire down");
    router.setStatus("closed");
    await expect(router.call({ method: "info.get" })).rejects.toThrow("connection to core closed");
  });
});

/** A socket the test opens, feeds and closes by hand. */
class FakeSocket implements SocketLike {
  sent: string[] = [];
  private handlers = new Map<string, ((event: { data: unknown }) => void)[]>();
  send(data: string) {
    this.sent.push(data);
  }
  addEventListener(type: string, listener: (event: { data: unknown }) => void) {
    this.handlers.set(type, [...(this.handlers.get(type) ?? []), listener]);
  }
  emit(type: "open" | "close" | "message", data: unknown = undefined) {
    for (const handler of this.handlers.get(type) ?? []) handler({ data });
  }
}

const INFO = { type: "info", data: { name: "ovra", version: "0.1.0", mode: "test" } };

describe("connectSocket", () => {
  let sockets: FakeSocket[];
  let statuses: ConnectionStatus[];
  let router: FrameRouter;
  let serverUp: boolean;
  const last = () => sockets[sockets.length - 1] as FakeSocket;

  beforeEach(() => {
    vi.useFakeTimers();
    sockets = [];
    statuses = [];
    serverUp = false;
    router = connectSocket(
      () => {
        const socket = new FakeSocket();
        sockets.push(socket);
        return socket;
      },
      async () => serverUp,
    );
    router.onStatus((status) => statuses.push(status));
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("queues calls while connecting and sends them on open", async () => {
    const reply = router.call({ method: "info.get" });
    expect(last().sent).toEqual([]);
    last().emit("open");
    expect(last().sent).toEqual([JSON.stringify({ id: 1, method: "info.get" })]);
    last().emit("message", JSON.stringify({ id: 1, result: INFO }));
    await expect(reply).resolves.toEqual(INFO);
    expect(statuses).toEqual(["connecting", "open"]);
  });

  it("fails pending calls on close and new calls until it reconnects", async () => {
    last().emit("open");
    const pending = router.call({ method: "info.get" });
    last().emit("close");
    await expect(pending).rejects.toThrow("connection to core closed");
    await expect(router.call({ method: "info.get" })).rejects.toThrow("connection to core closed");
    expect(statuses).toEqual(["connecting", "open", "closed"]);

    vi.advanceTimersByTime(250);
    expect(sockets).toHaveLength(2);
    expect(statuses.at(-1)).toBe("connecting");
    const queued = router.call({ method: "info.get" });
    last().emit("open");
    last().emit("message", JSON.stringify({ id: 2, result: INFO }));
    await expect(queued).resolves.toEqual(INFO);
  });

  it("backs off exponentially up to 10 s and resets after an open", async () => {
    const delays: number[] = [];
    for (let attempt = 0; attempt < 8; attempt++) {
      last().emit("close");
      delays.push(await nextDelay(sockets.length));
    }
    expect(delays).toEqual([250, 500, 1000, 2000, 4000, 8000, 10_000, 10_000]);

    last().emit("open");
    last().emit("close");
    expect(await nextDelay(sockets.length)).toBe(250);
  });

  it("tells every listener to reload after a reconnect, not after the first open", async () => {
    const events: string[] = [];
    router.onEvent((event) => events.push(event.type));
    last().emit("open");
    expect(events).toEqual([]);
    last().emit("close");
    await nextDelay(sockets.length);
    last().emit("open");
    expect(events).toEqual(["info.resync"]);
  });

  it("stops retrying when the server is up but keeps refusing the socket", async () => {
    serverUp = true;
    last().emit("close");
    await nextDelay(sockets.length);
    last().emit("close");
    await vi.waitFor(() => expect(statuses.at(-1)).toBe("refused"));
    await vi.advanceTimersByTimeAsync(60_000);
    expect(sockets).toHaveLength(2);
    await expect(router.call({ method: "info.get" })).rejects.toThrow("connection to core closed");
  });

  it("keeps retrying while the server does not answer at all", async () => {
    for (let attempt = 0; attempt < 4; attempt++) {
      last().emit("close");
      await nextDelay(sockets.length);
    }
    expect(statuses).not.toContain("refused");
  });

  /** Advances the clock until a new socket opens and returns how long that took. */
  async function nextDelay(count: number): Promise<number> {
    let waited = 0;
    while (sockets.length === count) {
      await vi.advanceTimersByTimeAsync(50);
      waited += 50;
    }
    return waited;
  }
});
