import { type ChildProcess, execFileSync, spawn } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "../..");

/** The user's real `claude`: OVRA_REAL_CLAUDE or the first one on PATH. */
function realClaude(): string {
  const chosen = process.env.OVRA_REAL_CLAUDE;
  if (chosen) return chosen;
  try {
    return execFileSync("sh", ["-c", "command -v claude"], { encoding: "utf8" }).trim();
  } catch {
    throw new Error("claude is not on PATH; install Claude Code and sign in, or set OVRA_REAL_CLAUDE");
  }
}

/**
 * The environment a person's own shell would give Ovra: without the variables
 * of a Claude Code session this suite may itself run in, which change what the
 * CLI prints (partial messages, host tools, session links).
 */
function userEnv(): NodeJS.ProcessEnv {
  const env: NodeJS.ProcessEnv = {};
  for (const [key, value] of Object.entries(process.env)) {
    if (/^(CLAUDE|CCR_)/.test(key) || key === "AI_AGENT" || key === "MAX_THINKING_TOKENS") continue;
    env[key] = value;
  }
  // Recordings and assertions do not depend on the model; a small one is faster.
  const model = process.env.OVRA_LIVE_MODEL ?? "haiku";
  if (model !== "default") env.ANTHROPIC_MODEL = model;
  return env;
}

/**
 * Starts ovra-daemon (real agent runtime, no --fake-agent) with a recording
 * stand-in for `claude` first on PATH.
 */
export default async function globalSetup(): Promise<() => void> {
  if (process.platform === "win32") throw new Error("the live agent suite runs on Linux and macOS");
  const bin = resolve(root, "target/debug/ovra-daemon");
  if (!existsSync(bin)) throw new Error(`${bin} not found; run \`just build-headless\` first`);

  const live = mkdtempSync(join(tmpdir(), "ovra-live-"));
  const shims = join(live, "bin");
  mkdirSync(shims);
  const shim = join(shims, "claude");
  writeFileSync(shim, `#!/bin/sh\nexec "${process.execPath}" "${join(here, "recorder.ts")}" "$@"\n`);
  chmodSync(shim, 0o755);
  const recordings = join(here, "test-results", "recordings");
  rmSync(recordings, { recursive: true, force: true });
  mkdirSync(recordings, { recursive: true });

  const env = userEnv();
  env.PATH = `${shims}${delimiter}${env.PATH ?? ""}`;
  env.OVRA_REAL_CLAUDE = realClaude();
  env.OVRA_RECORD_DIR = recordings;
  const daemon: ChildProcess = spawn(bin, ["--ui-dir", resolve(root, "ui/dist"), "--data-dir", join(live, "data")], {
    stdio: ["ignore", "pipe", "inherit"],
    env,
  });

  process.env.OVRA_URL = await new Promise<string>((ok, fail) => {
    const timer = setTimeout(() => fail(new Error("ovra-daemon did not print OVRA_URL in 10 s")), 10_000);
    let buffer = "";
    daemon.stdout?.on("data", (chunk: Buffer) => {
      buffer += chunk.toString();
      const match = buffer.match(/^OVRA_URL=(\S+)$/m);
      if (match?.[1]) {
        clearTimeout(timer);
        ok(match[1]);
      }
    });
    daemon.on("exit", (code) => fail(new Error(`ovra-daemon exited early with code ${code}`)));
  });
  process.env.OVRA_LIVE_DIR = live;
  process.env.OVRA_RECORD_DIR = recordings;
  process.env.OVRA_LIVE_CLAUDE = env.OVRA_REAL_CLAUDE;

  return () => {
    daemon.kill();
    rmSync(live, { recursive: true, force: true });
  };
}
