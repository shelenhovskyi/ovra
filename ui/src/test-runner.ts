// Vitest's runner, which also records each test's CPU time (see
// `test.runner` in vite.config.ts). The UI test budget
// (scripts/test-budgets.json) is checked against it, not wall time, so load
// from other work on the machine does not fail it. It spans the test and all
// its hooks, and works however files share modules, unlike hooks in a setup
// file, which run once for files that share it. The JSON report carries it.
import { type RunnerTestCase, TestRunner } from "vitest";

declare module "vitest" {
  interface TaskMeta {
    /** CPU time of the worker thread during the test and its hooks, in ms. */
    cpuMs?: number;
  }
}

export default class CpuTimingRunner extends TestRunner {
  #start = process.threadCpuUsage();

  override onBeforeRunTask(test: RunnerTestCase) {
    this.#start = process.threadCpuUsage();
    return super.onBeforeRunTask(test);
  }

  override onAfterRunTask(test: RunnerTestCase) {
    const { user, system } = process.threadCpuUsage(this.#start);
    test.meta.cpuMs = (user + system) / 1000;
    return super.onAfterRunTask(test);
  }
}
