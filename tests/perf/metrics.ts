// Records benchmark results and checks them against targets.json. Results go
// to test-results/metrics.jsonl; global-teardown.ts turns them into a table.
import { appendFileSync, existsSync, mkdirSync, readFileSync, rmSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect } from "@playwright/test";

const HERE = dirname(fileURLToPath(import.meta.url));
export const RESULTS = join(HERE, "test-results/metrics.jsonl");

export interface Target {
  max: number;
  source: "concept" | "budget";
  label: string;
}
export const TARGETS: Record<string, Target> = JSON.parse(readFileSync(join(HERE, "targets.json"), "utf8")).metrics;

export interface Result {
  name: string;
  value: number | null;
}

export function resetResults(): void {
  rmSync(RESULTS, { force: true });
  mkdirSync(dirname(RESULTS), { recursive: true });
}

/** Records a metric and fails the test (softly, so the others still run) when it misses its target. */
export function report(name: string, value: number | null): void {
  const target = TARGETS[name];
  if (!target) throw new Error(`${name} is not listed in targets.json`);
  appendFileSync(RESULTS, `${JSON.stringify({ name, value } satisfies Result)}\n`);
  if (value === null) return;
  expect.soft(value, `${name} (${target.label}) is over its target`).toBeLessThanOrEqual(target.max);
}

export function readResults(): Result[] {
  if (!existsSync(RESULTS)) return [];
  return readFileSync(RESULTS, "utf8")
    .split("\n")
    .filter(Boolean)
    .map((line) => JSON.parse(line) as Result);
}
