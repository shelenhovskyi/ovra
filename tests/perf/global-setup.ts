import { existsSync } from "node:fs";
import { daemonBin, REPO } from "./daemon";
import { resetResults } from "./metrics";
import { ensureRepo } from "./repo";

/** Generates (or reuses) the 100k-file repository before any benchmark runs. */
export default function globalSetup(): void {
  if (!existsSync(daemonBin())) throw new Error(`${daemonBin()} not found; run \`just perf\``);
  resetResults();
  const started = performance.now();
  ensureRepo(REPO);
  console.log(`perf repository ready in ${Math.round(performance.now() - started)} ms: ${REPO}`);
}
