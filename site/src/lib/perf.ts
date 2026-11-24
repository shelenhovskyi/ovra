// Speed gate results for the Speed page. The Pages workflow downloads the
// `perf-results` artifacts of recent CI runs on main into perf-data/<run id>/
// (metrics.jsonl from tests/perf plus a meta.json); targets come from the
// repository. Without downloaded data the page says so instead of inventing numbers.
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

// Builds run from site/ (pnpm --filter @ovra/site build).
const ROOT = process.cwd();
const DATA = join(ROOT, "perf-data");

export interface Target {
  max: number;
  source: "concept" | "budget";
  label: string;
}

export interface Run {
  run: number;
  sha: string;
  at: string;
  url: string;
  values: Record<string, number | null>;
}

export const targets: Record<string, Target> = JSON.parse(
  readFileSync(join(ROOT, "../tests/perf/targets.json"), "utf8"),
).metrics;

/** Runs with results, newest first. */
export function loadRuns(): Run[] {
  if (!existsSync(DATA)) return [];
  const runs: Run[] = [];
  for (const dir of readdirSync(DATA)) {
    const metaFile = join(DATA, dir, "meta.json");
    const metricsFile = join(DATA, dir, "metrics.jsonl");
    if (!existsSync(metaFile) || !existsSync(metricsFile)) continue;
    const meta = JSON.parse(readFileSync(metaFile, "utf8"));
    const values: Record<string, number | null> = {};
    for (const line of readFileSync(metricsFile, "utf8").split("\n")) {
      if (!line.trim()) continue;
      const { name, value } = JSON.parse(line) as { name: string; value: number | null };
      values[name] = value;
    }
    if (Object.keys(values).length > 0) runs.push({ ...meta, values });
  }
  return runs.sort((a, b) => b.at.localeCompare(a.at));
}

export const fmt = (n: number): string => (n >= 100 || Number.isInteger(n) ? String(Math.round(n)) : n.toFixed(1));

/** The unit shown after a value, from the metric name's suffix. */
export function unit(name: string): string {
  if (name.endsWith("_ms")) return "ms";
  if (name.endsWith("_mb")) return "MB";
  return "";
}
