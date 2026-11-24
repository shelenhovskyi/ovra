// Stand-in for the parts of the core that are still stubs on `main`, so a UX
// review can use the real UI end to end. It answers `projects.*`, `fs.*` and
// `open.external` from real folders on disk and forwards every other frame to
// the real headless core. Mirrors the core's rules: dirs first, then by name;
// `.git` and gitignored entries are `ignored`; git status folds into dirs.

import { execFileSync } from "node:child_process";
import {
  type Dirent,
  type FSWatcher,
  readdirSync,
  readFileSync,
  type Stats,
  statSync,
  watch,
  writeFileSync,
} from "node:fs";
import { basename, join, relative, sep } from "node:path";

type Project = { id: string; name: string; root: string; last_opened_ms: number | null };
type GitStatus = "modified" | "added" | "deleted" | "renamed" | "untracked" | "conflicted";
type Entry = { name: string; kind: "file" | "dir" | "symlink"; ignored: boolean; git: GitStatus | null };
type Frame = { id: number; method: string; params?: Record<string, unknown> };
type Reply = { result: unknown } | { error: { code: string; message: string } };

const MAX_FILE_BYTES = 5 * 1024 * 1024;
const RANK: GitStatus[] = ["untracked", "added", "renamed", "modified", "deleted", "conflicted"];

class Fail extends Error {
  readonly code: string;
  constructor(code: string, message: string) {
    super(message);
    this.code = code;
  }
}

function gitStatus(root: string): Map<string, GitStatus> {
  const out = new Map<string, GitStatus>();
  let raw = "";
  try {
    raw = execFileSync("git", ["status", "--porcelain=v1", "-z", "-uall"], { cwd: root, encoding: "utf8" });
  } catch {
    return out;
  }
  const parts = raw.split("\0");
  for (let i = 0; i < parts.length; i++) {
    const item = parts[i];
    if (!item || item.length < 4) continue;
    const xy = item.slice(0, 2);
    const path = item.slice(3);
    let status: GitStatus = "modified";
    if (xy === "??") status = "untracked";
    else if (xy.includes("U") || xy === "AA" || xy === "DD") status = "conflicted";
    else if (xy.includes("D")) status = "deleted";
    else if (xy.includes("R")) {
      status = "renamed";
      i++; // the original path follows
    } else if (xy.includes("A")) status = "added";
    out.set(path, status);
  }
  return out;
}

function ignoredNames(root: string, dir: string, names: string[]): Set<string> {
  const paths = names.map((n) => (dir ? `${dir}/${n}` : n));
  try {
    const out = execFileSync("git", ["check-ignore", "-z", "--stdin"], {
      cwd: root,
      input: paths.join("\0"),
      encoding: "utf8",
    });
    return new Set(
      out
        .split("\0")
        .filter(Boolean)
        .map((p) => basename(p)),
    );
  } catch {
    return new Set(); // exit code 1: nothing ignored
  }
}

function checkPath(path: unknown): string {
  if (typeof path !== "string") throw new Fail("bad_request", "path must be a string");
  if (path.startsWith("/") || path.split("/").some((p) => p === ".." || p === "."))
    throw new Fail("forbidden", `path leaves the project: ${path}`);
  return path;
}

export type FakeLog = (line: string) => void;

export class FakeCore {
  private projects: Project[] = [];
  private watchers: FSWatcher[] = [];
  private listeners = new Set<(event: unknown) => void>();
  private readonly log: FakeLog;

  constructor(log: FakeLog = () => {}) {
    this.log = log;
  }

  /** Registers a folder as a known project, as if added earlier. */
  seed(root: string, lastOpenedMs: number | null = null): Project {
    const project = { id: `p${this.projects.length + 1}`, name: basename(root), root, last_opened_ms: lastOpenedMs };
    this.projects.push(project);
    this.watch(project);
    return project;
  }

  onEvent(listener: (event: unknown) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  close(): void {
    for (const w of this.watchers) w.close();
  }

  /** Answers a frame, or returns null when the real core should handle it. */
  handle(frame: Frame): Reply | null {
    if (!/^(projects|fs|open)\./.test(frame.method)) return null;
    try {
      return { result: this.answer(frame) };
    } catch (err) {
      const code = err instanceof Fail ? err.code : "internal";
      return { error: { code, message: err instanceof Error ? err.message : String(err) } };
    }
  }

  private project(id: unknown): Project {
    const project = this.projects.find((p) => p.id === id);
    if (!project) throw new Fail("not_found", `no such project: ${String(id)}`);
    return project;
  }

  private answer({ method, params = {} }: Frame): unknown {
    switch (method) {
      case "projects.list":
        return { type: "projects.list", data: this.sorted() };
      case "projects.open": {
        const project = this.project(params.id);
        project.last_opened_ms = Date.now();
        return { type: "projects.project", data: project };
      }
      case "projects.add": {
        const root = String(params.root ?? "").replace(/\/+$/, "");
        let isDir = false;
        try {
          isDir = statSync(root).isDirectory();
        } catch {
          throw new Fail("not_found", `no such folder: ${root}`);
        }
        if (!isDir) throw new Fail("bad_request", `${root} is not a folder`);
        return { type: "projects.project", data: this.projects.find((p) => p.root === root) ?? this.seed(root) };
      }
      case "projects.remove":
        this.project(params.id);
        this.projects = this.projects.filter((p) => p.id !== params.id);
        return { type: "projects.list", data: this.sorted() };
      case "fs.list_dir":
        return { type: "fs.dir", data: this.list(this.project(params.project), checkPath(params.path)) };
      case "fs.read_file":
        return { type: "fs.file", data: this.read(this.project(params.project), checkPath(params.path)) };
      case "fs.write_file": {
        const project = this.project(params.project);
        const path = checkPath(params.path);
        const full = join(project.root, path);
        const expected = params.expected_mtime_ms;
        if (typeof expected === "number" && Math.trunc(statSync(full).mtimeMs) !== expected)
          throw new Fail("conflict", `${path} changed on disk`);
        writeFileSync(full, String(params.text ?? ""));
        return { type: "fs.written", data: { mtime_ms: Math.trunc(statSync(full).mtimeMs) } };
      }
      case "open.external":
        this.log(`open.external ${JSON.stringify(params)} (not really opened)`);
        return { type: "ok" };
      default:
        throw new Fail("bad_request", `unknown method ${method}`);
    }
  }

  private sorted(): Project[] {
    return [...this.projects].sort((a, b) => (b.last_opened_ms ?? 0) - (a.last_opened_ms ?? 0));
  }

  private list(project: Project, path: string): { path: string; entries: Entry[] } {
    const dir = join(project.root, path);
    let dirents: Dirent[];
    try {
      if (!statSync(dir).isDirectory()) throw new Fail("bad_request", `${path} is not a directory`);
      dirents = readdirSync(dir, { withFileTypes: true });
    } catch (err) {
      if (err instanceof Fail) throw err;
      throw new Fail("not_found", `no such directory: ${path}`);
    }
    const names = dirents.map((d) => d.name);
    const ignored = ignoredNames(project.root, path, names);
    const status = gitStatus(project.root);
    const prefix = path ? `${path}/` : "";
    const entries: Entry[] = dirents.map((d) => {
      const kind = d.isSymbolicLink() ? "symlink" : d.isDirectory() ? "dir" : "file";
      const full = prefix + d.name;
      let git: GitStatus | null = status.get(full) ?? null;
      if (kind === "dir") {
        for (const [p, s] of status)
          if (p.startsWith(`${full}/`) && (!git || RANK.indexOf(s) > RANK.indexOf(git))) git = s;
      }
      return { name: d.name, kind, ignored: d.name === ".git" || ignored.has(d.name), git };
    });
    entries.sort((a, b) => Number(a.kind !== "dir") - Number(b.kind !== "dir") || (a.name < b.name ? -1 : 1));
    return { path, entries };
  }

  private read(project: Project, path: string): unknown {
    const full = join(project.root, path);
    let st: Stats;
    try {
      st = statSync(full);
    } catch {
      throw new Fail("not_found", `no such file: ${path}`);
    }
    if (st.isDirectory()) throw new Fail("bad_request", `${path} is a directory`);
    const base = { path, size: st.size, mtime_ms: Math.trunc(st.mtimeMs) };
    if (st.size > MAX_FILE_BYTES) return { ...base, body: { kind: "too_large" } };
    const bytes = readFileSync(full);
    const text = bytes.toString("utf8");
    if (!text.includes("�") && !bytes.includes(0)) return { ...base, body: { kind: "text", text } };
    const ext = path.split(".").pop() ?? "";
    const mime =
      { png: "image/png", jpg: "image/jpeg", gif: "image/gif", svg: "image/svg+xml" }[ext] ??
      "application/octet-stream";
    return { ...base, body: { kind: "binary", mime, base64: bytes.toString("base64") } };
  }

  private watch(project: Project): void {
    let pending = new Set<string>();
    let timer: ReturnType<typeof setTimeout> | undefined;
    const flush = (): void => {
      const paths = [...pending];
      pending = new Set();
      const git = paths.some((p) => p === ".git" || p.startsWith(".git/"));
      const changed = paths.filter((p) => !(p === ".git" || p.startsWith(".git/")));
      if (changed.length) this.emit({ type: "fs.changed", data: { project: project.id, paths: changed } });
      if (git || changed.length) this.emit({ type: "fs.git_changed", data: { project: project.id } });
    };
    const watcher = watch(project.root, { recursive: true }, (_kind, file) => {
      if (!file) return;
      pending.add(relative(project.root, join(project.root, file.toString())).split(sep).join("/"));
      clearTimeout(timer);
      timer = setTimeout(flush, 50);
    });
    this.watchers.push(watcher);
  }

  private emit(event: unknown): void {
    for (const listener of this.listeners) listener(event);
  }
}
