// This phone's identity for Ovra (ADR 0014): a P-256 key made here with
// WebCrypto, whose private half can never leave the browser
// (`extractable: false`), and the device id the computer gave at pairing.
// There is no token: every connection signs a fresh challenge.

/** What the computer knows this phone by. */
export type Identity = {
  device: string;
  /** The name kept on the computer at pairing. */
  name: string;
  /** The origin it was paired with; a key signs for that origin only. */
  origin: string;
  key: CryptoKeyPair;
};

/** Where the identity is kept: IndexedDB in the app, memory in tests. */
export interface IdentityStore {
  load(): Promise<Identity | null>;
  save(identity: Identity): Promise<void>;
  clear(): Promise<void>;
}

const ALGORITHM = { name: "ECDSA", namedCurve: "P-256" } as const;

export function base64url(bytes: ArrayBuffer | Uint8Array): string {
  const view = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
  let text = "";
  for (const byte of view) text += String.fromCharCode(byte);
  return btoa(text).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

export function fromBase64url(text: string): Uint8Array<ArrayBuffer> {
  const padded = text
    .replace(/-/g, "+")
    .replace(/_/g, "/")
    .padEnd(Math.ceil(text.length / 4) * 4, "=");
  const raw = atob(padded);
  const bytes = new Uint8Array(raw.length);
  for (let i = 0; i < raw.length; i++) bytes[i] = raw.charCodeAt(i);
  return bytes;
}

/** A new key; the private half cannot be exported, not even by this page. */
export function makeKey(): Promise<CryptoKeyPair> {
  return crypto.subtle.generateKey(ALGORITHM, false, ["sign", "verify"]);
}

/** The public key as the computer keeps it: the uncompressed point (65 bytes), base64url. */
export async function publicKey(key: CryptoKeyPair): Promise<string> {
  return base64url(await crypto.subtle.exportKey("raw", key.publicKey));
}

/** What the phone signs for each connection; the server checks the same text (`crates/remote/src/auth.rs`). */
export function challengeText(origin: string, challenge: string): string {
  return `ovra-companion-v1\n${origin}\n${challenge}`;
}

/** An ECDSA P-256 / SHA-256 signature, raw r||s as WebCrypto makes it, base64url. */
export async function sign(key: CryptoKeyPair, text: string): Promise<string> {
  const signature = await crypto.subtle.sign(
    { name: "ECDSA", hash: "SHA-256" },
    key.privateKey,
    new TextEncoder().encode(text),
  );
  return base64url(signature);
}

/**
 * The four digits both screens show during pairing: SHA-256 of
 * `secret \n key`, first four bytes as a number, mod 10000 (`auth::code`).
 */
export async function pairingCode(secret: string, key: string): Promise<string> {
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(`${secret}\n${key}`)));
  const n = (((digest[0] ?? 0) << 24) | ((digest[1] ?? 0) << 16) | ((digest[2] ?? 0) << 8) | (digest[3] ?? 0)) >>> 0;
  return String(n % 10_000).padStart(4, "0");
}

/** A name to suggest for this phone; the person may change it. */
export function guessName(userAgent: string): string {
  if (/iPhone/.test(userAgent)) return "iPhone";
  if (/iPad/.test(userAgent)) return "iPad";
  if (/Android/.test(userAgent)) return "Android phone";
  return "Phone";
}

const DB = "ovra-phone";
const STORE = "identity";
const KEY = "this-phone";

function open(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DB, 1);
    request.onupgradeneeded = () => request.result.createObjectStore(STORE);
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

function run<T>(mode: IDBTransactionMode, work: (store: IDBObjectStore) => IDBRequest<T>): Promise<T> {
  return open().then((db) =>
    new Promise<T>((resolve, reject) => {
      const request = work(db.transaction(STORE, mode).objectStore(STORE));
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(request.error);
    }).finally(() => db.close()),
  );
}

/** IndexedDB keeps CryptoKey objects as they are, still non-extractable. */
export const browserStore: IdentityStore = {
  load: () => run<Identity | undefined>("readonly", (s) => s.get(KEY)).then((v) => v ?? null),
  save: (identity) => run("readwrite", (s) => s.put(identity, KEY)).then(() => undefined),
  clear: () => run("readwrite", (s) => s.delete(KEY)).then(() => undefined),
};

/** For tests. */
export function memoryStore(initial: Identity | null = null): IdentityStore {
  let kept = initial;
  return {
    load: () => Promise.resolve(kept),
    save: (identity) => {
      kept = identity;
      return Promise.resolve();
    },
    clear: () => {
      kept = null;
      return Promise.resolve();
    },
  };
}
