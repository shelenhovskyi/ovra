// Mirrors crates/core/src/protocol/secrets.rs.

import type { ProjectId } from "./projects";

/** A secret without its value: the netproxy adds it to requests for one host. */
export type SecretInfo = {
  /** 1 to 64 of `A-Z a-z 0-9 _ . -`, starting with a letter or digit. */
  name: string;
  /** One lowercase DNS name, optionally with `:port`. */
  host: string;
  /** The request header that carries it, such as `Authorization`. */
  header: string;
  /** Text before the value, such as `Bearer `. */
  prefix: string;
  /** The variable that holds a placeholder in the sandbox, such as `GH_TOKEN`, so tools start. */
  variable?: string;
};

/** Whether values can be stored; there is no fallback to a file. */
export type SecretStoreStatus = { state: "available"; name: string } | { state: "unavailable"; reason: string };

export type SecretList = { store: SecretStoreStatus; secrets: SecretInfo[] };

export type SecretsRequest =
  | { method: "secrets.list"; params: { project: ProjectId } }
  /** `bad_request` for a field that breaks the rules, `conflict` for a taken host and header. */
  | { method: "secrets.set"; params: { project: ProjectId; secret: SecretInfo; value: string } }
  | { method: "secrets.remove"; params: { project: ProjectId; name: string } };

export type SecretsResponse = { type: "secrets.list"; data: SecretList };

export type SecretsEvent = { type: "secrets.changed"; data: { project: ProjectId } };
