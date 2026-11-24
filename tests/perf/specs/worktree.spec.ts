// Snapshots, diffs and restores of crates/worktree on a large working tree:
// 2,000 tracked and 10,000 untracked files, 600 changed by an "agent".
// The release `bench` example of ovra-worktree does the work and prints
// one JSON line per metric.
import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "@playwright/test";
import { report } from "../metrics";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const BIN = resolve(ROOT, "target/release/examples", process.platform === "win32" ? "bench.exe" : "bench");

test("worktree snapshots and review diffs on 12k files", () => {
  if (!existsSync(BIN)) throw new Error(`${BIN} not found; run \`just perf\``);
  const out = execFileSync(BIN, { encoding: "utf8", timeout: 50_000 });
  for (const line of out.split("\n").filter(Boolean)) {
    const { name, value } = JSON.parse(line) as { name: string; value: number };
    report(name, value);
  }
});
