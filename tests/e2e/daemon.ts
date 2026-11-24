import { type ChildProcess, spawn } from "node:child_process";
import { existsSync, mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");

/** Starts ovra-daemon on a free port and exposes its URL as OVRA_URL. */
export default async function globalSetup(): Promise<() => void> {
  const exe = process.platform === "win32" ? "ovra-daemon.exe" : "ovra-daemon";
  const bin = resolve(root, "target/debug", exe);
  if (!existsSync(bin)) throw new Error(`${bin} not found; run \`just build-headless\` first`);

  const dataDir = mkdtempSync(join(tmpdir(), "ovra-e2e-"));
  const daemon: ChildProcess = spawn(bin, ["--ui-dir", resolve(root, "ui/dist"), "--data-dir", dataDir], {
    stdio: ["ignore", "pipe", "inherit"],
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

  return () => {
    daemon.kill();
  };
}
