// Mirrors crates/core/src/protocol/ssh.rs: remote machines over the
// person's own SSH (ADR 0016). Only the laptop's core answers these.

/** A remote machine the person added. */
export type RemoteHost = {
  id: string;
  /** What the person calls it; shown as "on <name>". */
  name: string;
  /** As typed in a terminal: a host alias or `user@host`. */
  destination: string;
  /** `ovra-daemon` on the remote machine. */
  daemon: string;
  /** SHA256 fingerprints of the pinned host keys. */
  fingerprints: string[];
  /** Milliseconds since the Unix epoch. */
  last_connected?: number;
};

export type NewRemoteHost = {
  name: string;
  destination: string;
  /** `ovra-daemon` when absent. */
  daemon?: string;
  fingerprints: string[];
};

export type HostKey = {
  /** E.g. `ssh-ed25519`. */
  kind: string;
  /** `SHA256:...`. */
  fingerprint: string;
};

/** What `ssh -G` resolves for a destination, and the keys found for it. */
export type PreparedHost = {
  destination: string;
  host_name: string;
  port: number;
  user: string;
  /** The name `ssh` checks keys under. */
  key_name: string;
  /** Empty: the person connects once in a terminal first. */
  keys: HostKey[];
};

export type SshRequest =
  | { method: "ssh.list" }
  /** What `ssh -G` resolves and the keys the person's `known_hosts` has. No SSH connection. */
  | { method: "ssh.prepare"; params: { destination: string } }
  /** Pins exactly `fingerprints`, which `ssh.prepare` returned and the person saw. */
  | { method: "ssh.add"; params: NewRemoteHost }
  | { method: "ssh.remove"; params: { id: string } }
  | { method: "ssh.rename"; params: { id: string; name: string } };

export type SshResponse =
  | { type: "ssh.hosts"; data: RemoteHost[] }
  | { type: "ssh.prepared"; data: PreparedHost }
  | { type: "ssh.host"; data: RemoteHost };

/** The list of remote machines changed. */
export type SshEvent = { type: "ssh.changed"; data: RemoteHost[] };

type SshMethod = SshRequest["method"];

/** The response type of each `ssh.*` method. */
export type SshResponseFor<M extends SshMethod> = M extends "ssh.list"
  ? Extract<SshResponse, { type: "ssh.hosts" }>
  : M extends "ssh.prepare"
    ? Extract<SshResponse, { type: "ssh.prepared" }>
    : M extends "ssh.add" | "ssh.rename"
      ? Extract<SshResponse, { type: "ssh.host" }>
      : { type: "ok" };
