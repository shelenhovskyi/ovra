import { expect, it } from "vitest";
import type { CoreEvent, Device, RemoteStatus, Request } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";
import { PhoneAccess, seen, timeLeft } from "./phone.svelte";

const OFF: RemoteStatus = { enabled: false, addresses: ["192.168.1.20"], connected: 0 };
const ON: RemoteStatus = { ...OFF, enabled: true, url: "https://192.168.1.20:52044" };
const PHONE: Device = { id: "d1", name: "iPhone", paired_at_ms: 1, connected: false };

function fake() {
  let status = OFF;
  let devices = [PHONE];
  const listeners: ((e: CoreEvent) => void)[] = [];
  const sent: Request[] = [];
  const transport: Transport = {
    call: ((request: Request) => {
      sent.push(request);
      switch (request.method) {
        case "remote.status":
          return Promise.resolve({ type: "remote.status", data: status });
        case "remote.enable":
          status = ON;
          return Promise.resolve({ type: "remote.status", data: status });
        case "remote.disable":
          status = OFF;
          return Promise.resolve({ type: "remote.status", data: status });
        case "remote.devices":
          return Promise.resolve({ type: "remote.devices", data: devices });
        case "remote.start_pairing":
          if (!status.enabled)
            return Promise.reject(new CoreError({ code: "conflict", message: "phone access is off" }));
          return Promise.resolve({
            type: "remote.pairing",
            data: {
              url: `${ON.url}/#pair=s`,
              qr_svg: "<svg/>",
              certificate_url: "http://192.168.1.20:52044/ca.crt",
              certificate_fingerprint: "AB12 CD34",
              expires_in: 300,
            },
          });
        case "remote.answer_pairing":
          return Promise.resolve({ type: "ok" });
        case "remote.forget":
          devices = [];
          return Promise.resolve({ type: "ok" });
        default:
          return Promise.reject(new Error(`unexpected ${request.method}`));
      }
    }) as Transport["call"],
    onEvent: (listener) => {
      listeners.push(listener);
      return () => listeners.splice(listeners.indexOf(listener), 1);
    },
  };
  const emit = (e: CoreEvent) => {
    for (const listener of listeners) listener(e);
  };
  return { transport, sent, emit };
}

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

it("turns on, pairs a phone the person allows and forgets it", async () => {
  const core = fake();
  const phone = new PhoneAccess(core.transport);
  const off = phone.follow();
  await settle();
  expect(phone.status?.enabled).toBe(false);
  expect(phone.devices.map((d) => d.name)).toEqual(["iPhone"]);

  await phone.startPairing();
  expect(phone.error).toBe("phone access is off");

  await phone.setEnabled(true, "192.168.1.20");
  expect(core.sent.at(-1)).toEqual({ method: "remote.enable", params: { address: "192.168.1.20" } });
  await phone.startPairing();
  expect(phone.error).toBeNull();
  expect(phone.pairing?.certificate_fingerprint).toBe("AB12 CD34");

  core.emit({ type: "remote.pairing_request", data: { id: "q1", name: "Pixel", code: "4821" } });
  expect(phone.request?.code).toBe("4821");
  await phone.answer(true);
  expect(core.sent.at(-1)).toEqual({ method: "remote.answer_pairing", params: { id: "q1", allow: true } });
  expect(phone.request).toBeNull();
  core.emit({ type: "remote.pairing_closed" });
  expect(phone.pairing).toBeNull();

  await phone.forget("d1");
  expect(phone.devices).toEqual([]);
  off();
});

it("closes the code when phone access stops", async () => {
  const core = fake();
  const phone = new PhoneAccess(core.transport);
  const off = phone.follow();
  await settle();
  await phone.setEnabled(true);
  expect(core.sent.at(-1)).toEqual({ method: "remote.enable", params: {} });
  await phone.startPairing();
  core.emit({ type: "remote.changed", data: OFF });
  expect(phone.pairing).toBeNull();
  expect(phone.status?.enabled).toBe(false);
  off();
});

it("says when a phone was last seen and how long a code works", () => {
  const now = 10 * 24 * 3600_000;
  expect(seen({ ...PHONE, connected: true }, now)).toBe("Connected");
  expect(seen(PHONE, now)).toBe("Not connected yet");
  expect(seen({ ...PHONE, seen_at_ms: now - 5 * 60_000 }, now)).toBe("Seen 5 min ago");
  expect(seen({ ...PHONE, seen_at_ms: now - 3 * 3600_000 }, now)).toBe("Seen 3 h ago");
  expect(seen({ ...PHONE, seen_at_ms: now - 24 * 3600_000 }, now)).toBe("Seen yesterday");
  expect(timeLeft(299)).toBe("4:59");
  expect(timeLeft(-3)).toBe("0:00");
});
