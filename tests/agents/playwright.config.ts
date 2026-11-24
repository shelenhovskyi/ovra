import { defineConfig } from "@playwright/test";

// Live suite: the real `claude` CLI, signed in, behind the headless daemon.
// Opt-in (`just agents-live`); never part of `just test` or `just e2e`, and
// not under the time budgets, because real model turns take seconds each.
export default defineConfig({
  testDir: "./specs",
  globalSetup: "./global-setup.ts",
  // Approvals are shared by all projects, and each test owns one session.
  workers: 1,
  timeout: 240_000,
  expect: { timeout: 90_000 },
  reporter: [["list"]],
  use: { trace: "off" },
});
