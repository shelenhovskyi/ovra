// Vitest setup for every UI test file (see `test.setupFiles` in vite.config.ts).
import { afterEach, beforeEach, vi } from "vitest";

declare module "vitest" {
  interface TaskMeta {
    /** CPU time of this worker thread during the test and its hooks, in ms. */
    cpuMs?: number;
  }
}

// `vi.waitFor` polls every 50 ms by default, so a condition that is not met on
// the first check costs a whole 50 ms. Poll every millisecond instead; the
// timeout stays the same.
const waitFor = vi.waitFor;
vi.waitFor = ((callback, options) =>
  waitFor(
    callback,
    typeof options === "number" ? { timeout: options, interval: 1 } : { interval: 1, ...options },
  )) as typeof vi.waitFor;

// The UI test budget (scripts/test-budgets.json) is checked against each
// test's CPU time, not its wall time, so load from other work on the machine
// does not fail it. This `beforeEach` runs before the file's own hooks and this
// `afterEach` after them, so the hooks count too. The JSON report carries it.
let cpuAtStart = process.threadCpuUsage();
beforeEach(() => {
  cpuAtStart = process.threadCpuUsage();
});
afterEach(({ task }) => {
  const { user, system } = process.threadCpuUsage(cpuAtStart);
  task.meta.cpuMs = (user + system) / 1000;
});
