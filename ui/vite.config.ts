import { svelte } from "@sveltejs/vite-plugin-svelte";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  build: { target: "es2022" },
  // Vitest runs in Node; without the browser condition Svelte resolves to its
  // server build and component tests cannot call mount().
  resolve: process.env.VITEST ? { conditions: ["browser"] } : undefined,
  test: {
    environment: "jsdom",
    // Hard per-test limit; budgets are in scripts/test-budgets.json.
    testTimeout: 1000,
    reporters: ["default", ["json", { outputFile: "test-results/vitest.json" }]],
  },
});
