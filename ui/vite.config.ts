import { svelte } from "@sveltejs/vite-plugin-svelte";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  build: { target: "es2022" },
  test: {
    environment: "jsdom",
    // Hard per-test limit; budgets are in scripts/test-budgets.json.
    testTimeout: 1000,
    reporters: ["default", ["json", { outputFile: "test-results/vitest.json" }]],
  },
});
