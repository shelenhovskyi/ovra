import { type ChildProcess, spawn } from "node:child_process";
import { existsSync, mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");

/** A built binary in `target/debug`. */
export function binary(name: string): string {
  const path = resolve(root, "target/debug", process.platform === "win32" ? `${name}.exe` : name);
  if (!existsSync(path)) throw new Error(`${path} not found; run \`just build-headless\` first`);
  return path;
}

/** Starts ovra-daemon on a free port; resolves with its URL and a stop function. */
export async function startDaemon(
  args: string[],
  env: NodeJS.ProcessEnv = process.env,
): Promise<{ url: string; stop: () => void }> {
  const dataDir = mkdtempSync(join(tmpdir(), "ovra-e2e-"));
  const daemon: ChildProcess = spawn(
    binary("ovra-daemon"),
    ["--ui-dir", resolve(root, "ui/dist"), "--data-dir", dataDir, ...args],
    { stdio: ["ignore", "pipe", "inherit"], env },
  );
  const url = await new Promise<string>((ok, fail) => {
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
  return { url, stop: () => daemon.kill() };
}

/** Starts the shared daemon with the scripted fake agent; exposes OVRA_URL. */
export default async function globalSetup(): Promise<() => void> {
  const { url, stop } = await startDaemon([
    "--fake-agent",
    "--fake-servers",
    "--fake-browser",
    "--fake-forge",
    "--fake-ssh",
  ]);
  process.env.OVRA_URL = url;
  return stop;
}
