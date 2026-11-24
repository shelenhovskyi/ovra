// Starts release builds of ovra-daemon and talks to them over WebSocket the
// way the UI does, so benchmarks measure the real wiring.
import { type ChildProcess, spawn } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
/** The generated repository (see repo.ts and global-setup.ts). */
export const REPO = join(ROOT, "target/perf/repo-100k");

export function daemonBin(): string {
  const exe = process.platform === "win32" ? "ovra-daemon.exe" : "ovra-daemon";
  return resolve(ROOT, "target/release", exe);
}

export interface Daemon {
  url: string;
  /** Milliseconds from spawn to the printed OVRA_URL. */
  readyMs: number;
  pid: number;
  stop(): Promise<void>;
}

export async function startDaemon(dataDir = mkdtempSync(join(tmpdir(), "ovra-perf-"))): Promise<Daemon> {
  const bin = daemonBin();
  if (!existsSync(bin)) throw new Error(`${bin} not found; run \`just perf\``);
  const started = performance.now();
  const child: ChildProcess = spawn(bin, ["--ui-dir", resolve(ROOT, "ui/dist"), "--data-dir", dataDir], {
    stdio: ["ignore", "pipe", "inherit"],
    env: { ...process.env, RUST_LOG: "warn" },
  });
  const url = await new Promise<string>((ok, fail) => {
    const timer = setTimeout(() => fail(new Error("ovra-daemon did not print OVRA_URL in 10 s")), 10_000);
    let buffer = "";
    child.stdout?.on("data", (chunk: Buffer) => {
      buffer += chunk.toString();
      const match = buffer.match(/^OVRA_URL=(\S+)$/m);
      if (match?.[1]) {
        clearTimeout(timer);
        ok(match[1]);
      }
    });
    child.on("exit", (code) => fail(new Error(`ovra-daemon exited early with code ${code}`)));
  });
  const readyMs = performance.now() - started;
  const pid = child.pid;
  if (pid === undefined) throw new Error("ovra-daemon has no pid");
  return {
    url,
    readyMs,
    pid,
    stop: () =>
      new Promise((done) => {
        if (child.exitCode !== null) return done();
        child.once("exit", () => done());
        child.kill();
      }),
  };
}

/** Resident and peak memory of a process in MB (Linux), or null elsewhere. */
export function memoryMb(pid: number): { rss: number; peak: number } | null {
  const path = `/proc/${pid}/status`;
  if (!existsSync(path)) return null;
  const status = readFileSync(path, "utf8");
  const kb = (key: string): number => Number(status.match(new RegExp(`^${key}:\\s+(\\d+) kB`, "m"))?.[1] ?? 0);
  return { rss: kb("VmRSS") / 1024, peak: kb("VmHWM") / 1024 };
}

/** RSS once it stops moving: two samples 100 ms apart within 1 MB, at most 3 s. */
export async function settledRssMb(pid: number): Promise<number | null> {
  const first = memoryMb(pid);
  if (first === null) return null;
  let last = first.rss;
  for (let i = 0; i < 30; i++) {
    await new Promise((r) => setTimeout(r, 100));
    const now: number = memoryMb(pid)?.rss ?? last;
    if (Math.abs(now - last) < 1) return now;
    last = now;
  }
  return last;
}

type Frame = { id?: number; result?: { type: string; data?: unknown }; error?: { message: string }; event?: Event };
export type Event = { type: string; data?: { project?: string; paths?: string[] } };

/** A WebSocket client for the core's JSON frames. */
export class Client {
  private next = 1;
  private pending = new Map<number, { ok: (v: unknown) => void; fail: (e: Error) => void }>();
  private waiters: { match: (e: Event) => boolean; ok: (e: Event) => void }[] = [];

  private constructor(private socket: WebSocket) {
    socket.addEventListener("message", (msg) => {
      const frame = JSON.parse(String(msg.data)) as Frame;
      if (frame.event) {
        const event = frame.event;
        const matched = this.waiters.filter((w) => w.match(event));
        this.waiters = this.waiters.filter((w) => !matched.includes(w));
        for (const w of matched) w.ok(event);
        return;
      }
      const call = frame.id === undefined ? undefined : this.pending.get(frame.id);
      if (!call || frame.id === undefined) return;
      this.pending.delete(frame.id);
      if (frame.error) call.fail(new Error(frame.error.message));
      else call.ok(frame.result?.data);
    });
  }

  static connect(url: string): Promise<Client> {
    const page = new URL(url);
    const ws = `ws://${page.host}/ws?token=${page.searchParams.get("token") ?? ""}`;
    return new Promise((ok, fail) => {
      const socket = new WebSocket(ws);
      socket.addEventListener("open", () => ok(new Client(socket)));
      socket.addEventListener("error", () => fail(new Error(`cannot connect to ${ws}`)));
    });
  }

  call<T>(method: string, params?: unknown): Promise<T> {
    const id = this.next++;
    return new Promise((ok, fail) => {
      this.pending.set(id, { ok: ok as (v: unknown) => void, fail });
      this.socket.send(JSON.stringify(params === undefined ? { id, method } : { id, method, params }));
    });
  }

  /** Resolves with the first event matching `match`, from now on. */
  nextEvent(match: (e: Event) => boolean, timeoutMs = 5_000): Promise<Event> {
    return new Promise((ok, fail) => {
      const timer = setTimeout(() => fail(new Error(`no matching event in ${timeoutMs} ms`)), timeoutMs);
      this.waiters.push({
        match,
        ok: (e) => {
          clearTimeout(timer);
          ok(e);
        },
      });
    });
  }

  close(): void {
    this.socket.close();
  }
}

/** Time of `work` in milliseconds. */
export async function timed(work: () => Promise<unknown>): Promise<number> {
  const started = performance.now();
  await work();
  return performance.now() - started;
}

export function median(values: number[]): number {
  const sorted = [...values].sort((a, b) => a - b);
  const mid = Math.floor(sorted.length / 2);
  return sorted.length % 2 ? (sorted[mid] ?? 0) : ((sorted[mid - 1] ?? 0) + (sorted[mid] ?? 0)) / 2;
}
