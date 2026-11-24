import { defineConfig } from "@playwright/test";

// The phase 1 speed gate: release ovra-daemon against a generated 100k-file
// repository. Not part of `just e2e`; run with `just perf`. Benchmarks run one
// at a time so they do not disturb each other's timings.
export default defineConfig({
  testDir: "./specs",
  globalSetup: "./global-setup.ts",
  globalTeardown: "./global-teardown.ts",
  workers: 1,
  fullyParallel: false,
  timeout: 60_000,
  // Includes generating the repository on a cold cache.
  globalTimeout: 600_000,
  reporter: [process.env.CI ? ["github"] : ["list"]],
  use: { trace: "off", viewport: { width: 1280, height: 800 } },
});
