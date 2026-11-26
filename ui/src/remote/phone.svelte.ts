// Phone access (`remote.*`, ADR 0014) as the desktop sees it: whether Ovra
// listens for phones, the open pairing code, the phone that waits for the
// person's answer, and the paired phones. Kept current from `remote.*` events.
import type { Device, Pairing, PairingRequest, RemoteStatus } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";

export class PhoneAccess {
  /** `null` until the core answered, or when this front end has no phone access. */
  status = $state<RemoteStatus | null>(null);
  devices = $state<Device[]>([]);
  /** The pairing code shown, while its window is open. */
  pairing = $state<Pairing | null>(null);
  /** A phone used the code and waits for Allow or Deny. */
  request = $state<PairingRequest | null>(null);
  error = $state<string | null>(null);

  constructor(private readonly transport: Transport) {}

  /** Loads the state and follows its changes; returns the unsubscribe. */
  follow(): () => void {
    const off = this.transport.onEvent((event) => {
      switch (event.type) {
        case "remote.changed":
          this.status = event.data;
          if (!event.data.url) this.closePairing();
          break;
        case "remote.pairing_request":
          this.request = event.data;
          break;
        case "remote.pairing_closed":
          this.closePairing();
          break;
        case "remote.devices_changed":
          void this.loadDevices();
          break;
      }
    });
    this.transport
      .call({ method: "remote.status" })
      .then((reply) => {
        this.status = reply.data;
        void this.loadDevices();
      })
      .catch(() => {
        // A front end without phone access shows nothing about it.
      });
    return off;
  }

  private closePairing(): void {
    this.pairing = null;
    this.request = null;
  }

  private async attempt(run: () => Promise<void>): Promise<void> {
    this.error = null;
    try {
      await run();
    } catch (err) {
      this.error = err instanceof CoreError ? err.error.message : String(err);
    }
  }

  async loadDevices(): Promise<void> {
    try {
      this.devices = (await this.transport.call({ method: "remote.devices" })).data;
    } catch {
      this.devices = [];
    }
  }

  setEnabled(on: boolean, address?: string): Promise<void> {
    return this.attempt(async () => {
      const reply = on
        ? await this.transport.call({ method: "remote.enable", params: address ? { address } : {} })
        : await this.transport.call({ method: "remote.disable" });
      this.status = reply.data;
      if (!on) this.closePairing();
    });
  }

  startPairing(): Promise<void> {
    return this.attempt(async () => {
      this.pairing = (await this.transport.call({ method: "remote.start_pairing" })).data;
    });
  }

  /** Allow or Deny for the waiting phone. */
  answer(allow: boolean): Promise<void> {
    const request = this.request;
    if (!request) return Promise.resolve();
    this.request = null;
    return this.attempt(async () => {
      await this.transport.call({ method: "remote.answer_pairing", params: { id: request.id, allow } });
    });
  }

  forget(device: string): Promise<void> {
    return this.attempt(async () => {
      await this.transport.call({ method: "remote.forget", params: { device } });
      this.devices = this.devices.filter((d) => d.id !== device);
    });
  }
}

/** "Connected", "Seen 5 min ago", "Never connected". */
export function seen(device: Device, now = Date.now()): string {
  if (device.connected) return "Connected";
  if (device.seen_at_ms === undefined) return "Not connected yet";
  const minutes = Math.round((now - device.seen_at_ms) / 60_000);
  if (minutes < 1) return "Seen just now";
  if (minutes < 60) return `Seen ${minutes} min ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `Seen ${hours} h ago`;
  const days = Math.round(hours / 24);
  return days === 1 ? "Seen yesterday" : `Seen ${days} days ago`;
}

/** The code's time left as m:ss. */
export function timeLeft(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}
