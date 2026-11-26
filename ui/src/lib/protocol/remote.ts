// Mirrors crates/core/src/protocol/remote.rs: phone access over the LAN
// (ADR 0014), from the desktop. A phone itself never uses these.

export type RemoteStatus = {
  enabled: boolean;
  /** The address phones open, while listening. */
  url?: string;
  /** This computer's private addresses that phone access may use. */
  addresses: string[];
  connected: number;
  /** Why phone access is not listening although it is on. */
  problem?: string;
};

export type Pairing = {
  /** What the QR code holds: the app's address with the one-time secret. */
  url: string;
  qr_svg: string;
  /** Where the phone gets Ovra's certificate first, over plain HTTP. */
  certificate_url: string;
  certificate_fingerprint: string;
  expires_in: number;
};

export type PairingRequest = { id: string; name: string; code: string };

export type Device = {
  id: string;
  name: string;
  paired_at_ms: number;
  seen_at_ms?: number;
  connected: boolean;
};

export type RemoteRequest =
  | { method: "remote.status" }
  | { method: "remote.enable"; params: { address?: string } }
  | { method: "remote.disable" }
  | { method: "remote.start_pairing" }
  | { method: "remote.answer_pairing"; params: { id: string; allow: boolean } }
  | { method: "remote.devices" }
  | { method: "remote.forget"; params: { device: string } };

export type RemoteResponse =
  | { type: "remote.status"; data: RemoteStatus }
  | { type: "remote.pairing"; data: Pairing }
  | { type: "remote.devices"; data: Device[] };

export type RemoteEvent =
  | { type: "remote.changed"; data: RemoteStatus }
  | { type: "remote.pairing_request"; data: PairingRequest }
  | { type: "remote.pairing_closed" }
  | { type: "remote.devices_changed" };

type RemoteMethod = RemoteRequest["method"];

/** The response type of each `remote.*` method. */
export type RemoteResponseFor<M extends RemoteMethod> = M extends "remote.status" | "remote.enable" | "remote.disable"
  ? Extract<RemoteResponse, { type: "remote.status" }>
  : M extends "remote.start_pairing"
    ? Extract<RemoteResponse, { type: "remote.pairing" }>
    : M extends "remote.devices"
      ? Extract<RemoteResponse, { type: "remote.devices" }>
      : { type: "ok" };
