import { appendFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { RESULTS, readResults, TARGETS } from "./metrics";

const fmt = (n: number): string => (n >= 100 ? String(Math.round(n)) : n.toFixed(1));

/** Prints every target with its result, and writes the same table for CI. */
export default function globalTeardown(): void {
  const results = new Map(readResults().map((r) => [r.name, r.value]));
  const lines = ["| Metric | Result | Target | Status |", "|---|---:|---:|---|"];
  let missed = 0;
  for (const [name, target] of Object.entries(TARGETS)) {
    const value = results.get(name);
    let status: string;
    if (value === undefined) status = "not run";
    else if (value === null) status = "n/a on this OS";
    else if (value <= target.max) status = "ok";
    else {
      missed++;
      status = `MISSED by ${fmt(value - target.max)} (${Math.round((value / target.max - 1) * 100)}%)`;
    }
    const shown = value === undefined || value === null ? "-" : fmt(value);
    lines.push(`| ${target.label} (\`${name}\`, ${target.source}) | ${shown} | ${fmt(target.max)} | ${status} |`);
  }
  const table = `## Phase 1 speed gate\n\n${lines.join("\n")}\n\n${missed} target(s) missed.\n`;
  console.log(`\n${table}`);
  writeFileSync(join(RESULTS, "../summary.md"), table);
  if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, table);
}
