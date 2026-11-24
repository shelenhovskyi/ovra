// Stands in for `claude` on the daemon's PATH during live tests: runs the
// real CLI unchanged and writes every stdin and stdout line of a session to
// OVRA_RECORD_DIR, as `{"ovra": ...}` and `{"cli": ...}` steps.
import { spawn } from "node:child_process";
import { createWriteStream } from "node:fs";
import { join } from "node:path";
import { createInterface } from "node:readline";

const real = process.env.OVRA_REAL_CLAUDE;
const dir = process.env.OVRA_RECORD_DIR;
if (!real || !dir) throw new Error("OVRA_REAL_CLAUDE and OVRA_RECORD_DIR must be set");
const args = process.argv.slice(2);

if (args[0] !== "-p") {
  // `--version`, `auth status`: nothing to record.
  const child = spawn(real, args, { stdio: "inherit" });
  child.on("exit", (code) => process.exit(code ?? 1));
} else {
  const log = createWriteStream(join(dir, `${Date.now()}-${process.pid}.jsonl`));
  log.write(`${JSON.stringify({ meta: { cwd: process.cwd(), args } })}\n`);
  const record = (side: "ovra" | "cli", line: string): void => {
    if (!line.trim()) return;
    try {
      log.write(`${JSON.stringify({ [side]: JSON.parse(line) })}\n`);
    } catch {
      log.write(`${JSON.stringify({ [`${side}_text`]: line })}\n`);
    }
  };
  const child = spawn(real, args, { stdio: ["pipe", "pipe", "inherit"] });
  createInterface({ input: process.stdin }).on("line", (line) => {
    record("ovra", line);
    child.stdin.write(`${line}\n`);
  });
  process.stdin.on("end", () => child.stdin.end());
  createInterface({ input: child.stdout }).on("line", (line) => {
    record("cli", line);
    process.stdout.write(`${line}\n`);
  });
  child.on("exit", (code) => {
    log.end(() => process.exit(code ?? 1));
  });
}
