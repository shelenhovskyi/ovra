// Local history: earlier versions of each file, kept on this computer, so a
// person can get back text that a save, an agent or another program
// replaced, also outside git. A version is kept when a file opens with text
// unlike its last one, when it is saved, and when it changes on disk while
// open. Kept in the page's IndexedDB, per project and path; files that look
// like secrets and large files are never kept.

import type { ProjectId } from "../lib/protocol";

/** Why a version was kept. */
export type VersionReason = "opened" | "saved" | "changed_on_disk";

export type Version = { project: ProjectId; path: string; time: number; text: string; reason: VersionReason };

/** Where versions live; newest first from `list`. */
export interface VersionStore {
  add(version: Version): Promise<void>;
  list(project: ProjectId, path: string): Promise<Version[]>;
  /** Drops all but the newest `keep` versions of the file. */
  prune(project: ProjectId, path: string, keep: number): Promise<void>;
}

/** Versions kept per file. */
export const VERSIONS_PER_FILE = 50;
/** Texts above this many UTF-16 units are not kept. */
const MAX_TEXT = 1024 * 1024;

/** File names whose text may be a secret: keys, tokens and passwords. */
const SECRET_NAMES =
  /^(\.env.*|.*\.(pem|key|p12|pfx|kdbx|tfvars|keystore|jks)|credentials.*|secrets?\..*|id_(rsa|dsa|ecdsa|ed25519).*|\.(npmrc|netrc|pgpass|pypirc|git-credentials|htpasswd))$/;

/** Names that may hold secrets (`.env*`, `*.pem`, `*.key`, `id_rsa`, `.npmrc`, ...), whose text stays out of the history. */
export function mayHoldSecrets(path: string): boolean {
  return SECRET_NAMES.test((path.split("/").at(-1) ?? "").toLowerCase());
}

/** Files whose newest text is remembered in memory; older ones are read again when saved. */
const REMEMBERED = 100;

export class LocalHistory {
  /** The newest text kept per file, so an unchanged save costs no read. */
  private readonly latest = new Map<string, string>();
  private queue: Promise<void> = Promise.resolve();

  constructor(
    private readonly store: VersionStore,
    private readonly now: () => number = Date.now,
  ) {}

  /** Keeps `text` as a version of the file unless it is the newest one already; never throws. */
  record(project: ProjectId, path: string, text: string, reason: VersionReason): Promise<void> {
    if (text.length > MAX_TEXT || mayHoldSecrets(path)) return this.queue;
    const time = this.now();
    // One at a time, so the newest-version check sees the version before.
    this.queue = this.queue.then(async () => {
      try {
        const key = `${project}\n${path}`;
        let newest = this.latest.get(key);
        if (newest === undefined) newest = (await this.store.list(project, path))[0]?.text;
        if (newest === text) return;
        await this.store.add({ project, path, time, text, reason });
        this.latest.delete(key);
        this.latest.set(key, text);
        // A Map keeps insertion order: the first key is the one used longest ago.
        if (this.latest.size > REMEMBERED) this.latest.delete(this.latest.keys().next().value as string);
        await this.store.prune(project, path, VERSIONS_PER_FILE);
      } catch {
        // No storage (private window, quota): the file works as before, without history.
      }
    });
    return this.queue;
  }

  /** The kept versions of a file, newest first. */
  async versions(project: ProjectId, path: string): Promise<Version[]> {
    await this.queue;
    try {
      return await this.store.list(project, path);
    } catch {
      return [];
    }
  }
}

/** Versions in memory, for tests and where the page has no IndexedDB. */
export class MemoryVersions implements VersionStore {
  readonly versions: Version[] = [];

  async add(version: Version): Promise<void> {
    this.versions.push(version);
  }

  async list(project: ProjectId, path: string): Promise<Version[]> {
    return this.versions.filter((v) => v.project === project && v.path === path).reverse();
  }

  async prune(project: ProjectId, path: string, keep: number): Promise<void> {
    const old = (await this.list(project, path)).slice(keep);
    for (const version of old) this.versions.splice(this.versions.indexOf(version), 1);
  }
}

const DB = "ovra-local-history";
const STORE = "versions";

/** Versions in the page's IndexedDB. */
class IndexedVersions implements VersionStore {
  private db: Promise<IDBDatabase> | null = null;

  private open(): Promise<IDBDatabase> {
    this.db ??= new Promise((resolve, reject) => {
      const request = indexedDB.open(DB, 1);
      request.onupgradeneeded = () => {
        const store = request.result.createObjectStore(STORE, { keyPath: "id", autoIncrement: true });
        store.createIndex("file", ["project", "path"]);
      };
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(request.error);
    });
    this.db.catch(() => {
      this.db = null;
    });
    return this.db;
  }

  private async run<T>(mode: IDBTransactionMode, work: (store: IDBObjectStore) => IDBRequest<T>): Promise<T> {
    const db = await this.open();
    return new Promise((resolve, reject) => {
      const request = work(db.transaction(STORE, mode).objectStore(STORE));
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(request.error);
    });
  }

  async add(version: Version): Promise<void> {
    await this.run("readwrite", (store) => store.add(version));
  }

  async list(project: ProjectId, path: string): Promise<Version[]> {
    const all = await this.run<Version[]>("readonly", (store) =>
      store.index("file").getAll(IDBKeyRange.only([project, path])),
    );
    // Keys grow with each add: the index lists oldest first.
    return all.reverse();
  }

  async prune(project: ProjectId, path: string, keep: number): Promise<void> {
    const ids = await this.run<IDBValidKey[]>("readonly", (store) =>
      store.index("file").getAllKeys(IDBKeyRange.only([project, path])),
    );
    const old = ids.slice(0, Math.max(0, ids.length - keep));
    if (old.length === 0) return;
    const db = await this.open();
    await new Promise<void>((resolve, reject) => {
      const tx = db.transaction(STORE, "readwrite");
      for (const id of old) tx.objectStore(STORE).delete(id);
      tx.oncomplete = () => resolve();
      tx.onerror = () => reject(tx.error);
    });
  }
}

/** The window's local history. */
export const localHistory = new LocalHistory(
  typeof indexedDB === "undefined" ? new MemoryVersions() : new IndexedVersions(),
);

/** "Saved", "Opened" or "Changed on disk", then the time: today's as 14:32, older ones with the date. */
export function versionLabel(version: Version, now = Date.now()): string {
  const what = { opened: "Opened", saved: "Saved", changed_on_disk: "Changed on disk" }[version.reason];
  const at = new Date(version.time);
  const time = at.toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit" });
  const sameDay = new Date(now).toDateString() === at.toDateString();
  const day = sameDay ? "today" : at.toLocaleDateString("en-GB", { day: "numeric", month: "short", year: "numeric" });
  return `${what} ${day} at ${time}`;
}
