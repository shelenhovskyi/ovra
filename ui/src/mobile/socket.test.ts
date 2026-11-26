import "./test-crypto";
import { afterEach, expect, it, vi } from "vitest";
import { fromBase64url, type Identity, makeKey } from "./identity";
import { connectPhone, type RawSocket, SignedSocket, socketUrl } from "./socket";

class FakeRaw implements RawSocket {
  sent: string[] = [];
  closed = false;
  private onMessage: ((e: { data: unknown }) => void) | null = null;
  private onClose: (() => void) | null = null;
  send(data: string) {
    this.sent.push(data);
  }
  close() {
    this.closed = true;
    this.onClose?.();
  }
  addEventListener(type: "close" | "message", listener: (e: { data: unknown }) => void) {
    if (type === "message") this.onMessage = listener;
    else this.onClose = listener as () => void;
  }
  receive(frame: unknown) {
    this.onMessage?.({ data: JSON.stringify(frame) });
  }
}

async function identity(): Promise<Identity> {
  return { device: "d1", name: "iPhone", origin: "https://192.168.1.20:52044", key: await makeKey() };
}

afterEach(() => {
  vi.useRealTimers();
});

it("opens only after the computer accepts the signed challenge", async () => {
  const id = await identity();
  const raw = new FakeRaw();
  const socket = new SignedSocket(raw, id, () => {});
  const events: string[] = [];
  socket.addEventListener("open", () => events.push("open"));
  socket.addEventListener("message", (e) => events.push(String(e.data)));
  socket.send("too early");
  raw.receive({ challenge: "c1" });
  await vi.waitFor(() => expect(raw.sent).toHaveLength(1));
  const proof = JSON.parse(raw.sent[0] ?? "") as { device: string; signature: string };
  expect(proof.device).toBe("d1");
  const ok = await crypto.subtle.verify(
    { name: "ECDSA", hash: "SHA-256" },
    id.key.publicKey,
    fromBase64url(proof.signature),
    new TextEncoder().encode("ovra-companion-v1\nhttps://192.168.1.20:52044\nc1"),
  );
  expect(ok).toBe(true);
  expect(events).toEqual([]);
  raw.receive({ paired: "iPhone" });
  await vi.waitFor(() => expect(events).toEqual(["open"]));
  raw.receive({ event: { type: "info.ready" } });
  await vi.waitFor(() => expect(events).toHaveLength(2));
  socket.send("frame");
  expect(raw.sent.at(-1)).toBe("frame");
});

it("tells the app when the computer forgot this phone and stops trying", async () => {
  vi.useFakeTimers();
  const id = await identity();
  const opened: FakeRaw[] = [];
  const unpaired = vi.fn();
  connectPhone(
    id,
    () => {
      const raw = new FakeRaw();
      opened.push(raw);
      return raw;
    },
    unpaired,
  );
  opened[0]?.receive({ challenge: "c1" });
  await vi.waitFor(() => expect(opened[0]?.sent).toHaveLength(1));
  opened[0]?.receive({ error: "not_paired" });
  await vi.waitFor(() => expect(unpaired).toHaveBeenCalledOnce());
  expect(opened[0]?.closed).toBe(true);
  await vi.advanceTimersByTimeAsync(60_000);
  expect(opened).toHaveLength(1);
});

it("closes on anything but the handshake it expects", async () => {
  const raw = new FakeRaw();
  new SignedSocket(raw, await identity(), () => {});
  raw.receive({ paired: "early" });
  expect(raw.closed).toBe(true);
  expect(socketUrl("https://192.168.1.20:52044")).toBe("wss://192.168.1.20:52044/ws");
  expect(socketUrl("http://127.0.0.1:7777")).toBe("ws://127.0.0.1:7777/ws");
});
