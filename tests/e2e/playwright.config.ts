import { defineConfig } from "@playwright/test";

// Runs against headless mode: `ovra-daemon` serves the built UI and the core.
// Build first: `just build-headless` (done by `just e2e`).
export default defineConfig({
  testDir: "./specs",
  globalSetup: "./daemon.ts",
  // One worker: the tests share one daemon, and the core watches only the
  // project opened last, so a test opening a project in parallel stops
  // another test's fs.changed events (a flaky conflict test in tabs.spec).
  workers: 1,
  // Hard limits; budgets are in scripts/test-budgets.json.
  timeout: 10_000,
  globalTimeout: 120_000,
  reporter: [process.env.CI ? ["github"] : ["list"], ["json", { outputFile: "test-results/report.json" }]],
  use: { trace: "off" },
});
