// The phone's connection to the computer: a WebSocket that proves this phone
// by signing the server's challenge, then carries the usual frames
// (`lib/transport.ts`). The server ends each one after 15 minutes; the
// transport opens a new one and the phone signs again.

import { connectSocket, type FrameRouter, type SocketLike } from "../lib/transport";
import { challengeText, type Identity, sign } from "./identity";

/** The part of `WebSocket` used here, so tests can pass a fake. */
export interface RawSocket {
  send(data: string): void;
  close(): void;
  addEventListener(type: "close", listener: () => void): void;
  addEventListener(type: "message", listener: (event: { data: unknown }) => void): void;
}

type Message = { data: unknown };

/**
 * A socket that reports `open` only once the computer accepted the
 * signature. `onUnpaired` runs when the computer no longer knows this phone.
 */
export class SignedSocket implements SocketLike {
  private step: "challenge" | "proof" | "open" | "closed" = "challenge";
  private readonly opens = new Set<() => void>();
  private readonly closes = new Set<() => void>();
  private readonly messages = new Set<(event: Message) => void>();

  constructor(
    private readonly raw: RawSocket,
    private readonly identity: Identity,
    private readonly onUnpaired: () => void,
  ) {
    raw.addEventListener("message", (event) => void this.receive(String(event.data)));
    raw.addEventListener("close", () => {
      if (this.step === "closed") return;
      this.step = "closed";
      for (const listener of this.closes) listener();
    });
  }

  send(data: string): void {
    if (this.step === "open") this.raw.send(data);
  }

  addEventListener(type: "open" | "close", listener: () => void): void;
  addEventListener(type: "message", listener: (event: Message) => void): void;
  addEventListener(type: "open" | "close" | "message", listener: (event: Message) => void): void {
    if (type === "message") this.messages.add(listener);
    else (type === "open" ? this.opens : this.closes).add(listener as () => void);
  }

  private async receive(text: string): Promise<void> {
    if (this.step === "open") {
      for (const listener of this.messages) listener({ data: text });
      return;
    }
    let frame: { challenge?: unknown; paired?: unknown; error?: unknown };
    try {
      frame = JSON.parse(text) as typeof frame;
    } catch {
      this.raw.close();
      return;
    }
    if (this.step === "challenge" && typeof frame.challenge === "string") {
      this.step = "proof";
      const signature = await sign(this.identity.key, challengeText(this.identity.origin, frame.challenge));
      this.raw.send(JSON.stringify({ device: this.identity.device, signature }));
    } else if (this.step === "proof" && typeof frame.paired === "string") {
      this.step = "open";
      for (const listener of this.opens) listener();
    } else {
      if (frame.error === "not_paired") this.onUnpaired();
      this.raw.close();
    }
  }
}

/** A socket that never opens: what the transport gets once this phone is unpaired, so it stops trying. */
const silent: SocketLike = { send: () => {}, addEventListener: () => {} };

/**
 * The transport for a paired phone. `open` makes the raw socket (a
 * `WebSocket` to `wss://<computer>/ws` in the app).
 */
export function connectPhone(identity: Identity, open: () => RawSocket, onUnpaired: () => void): FrameRouter {
  let unpaired = false;
  const lost = () => {
    if (unpaired) return;
    unpaired = true;
    onUnpaired();
  };
  const serverUp = () =>
    fetch(`${identity.origin}/`, { method: "HEAD", cache: "no-store" }).then(
      () => true,
      () => false,
    );
  return connectSocket(() => (unpaired ? silent : new SignedSocket(open(), identity, lost)), serverUp);
}

/** The WebSocket address of the computer at `origin`. */
export function socketUrl(origin: string): string {
  return `${origin.replace(/^http/, "ws")}/ws`;
}
