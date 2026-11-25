import { svelte } from "@sveltejs/vite-plugin-svelte";
import type { Plugin } from "vite";
import { defineConfig } from "vitest/config";

// The desktop window freezes Object.prototype (Tauri's `freezePrototype`), and
// xterm 6 assigns `toString` on a plain object at load, which then throws and
// leaves the window blank. Defining the property instead is equivalent and
// allowed. The build fails if the statement is gone, so an xterm update cannot
// silently drop or outgrow the fix; the e2e suite runs frozen to catch others.
function xtermUnderFrozenPrototype(): Plugin {
  const from = "o.toString=s;";
  const to = 'Object.defineProperty(o,"toString",{value:s,writable:!0,enumerable:!0,configurable:!0});';
  return {
    name: "ovra:xterm-frozen-prototype",
    enforce: "pre",
    transform(code, id) {
      if (!id.includes("/@xterm/xterm/lib/xterm.mjs")) return;
      if (!code.includes(from))
        this.error(`@xterm/xterm no longer contains \`${from}\`; recheck it under a frozen prototype`);
      return { code: code.replace(from, to), map: null };
    },
  };
}

export default defineConfig({
  plugins: [svelte(), xtermUnderFrozenPrototype()],
  // Pre-bundled dependencies skip plugin transforms, so the dev server would
  // serve xterm unfixed.
  optimizeDeps: { exclude: ["@xterm/xterm"] },
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  build: { target: "es2022" },
  // Vitest runs in Node; without the browser condition Svelte resolves to its
  // server build and component tests cannot call mount().
  resolve: process.env.VITEST ? { conditions: ["browser"] } : undefined,
  test: {
    environment: "jsdom",
    // One jsdom per worker instead of per file, with each file still in its own
    // module context: far less setup work competing with the tests themselves.
    pool: "vmThreads",
    // Two workers: each test's CPU time counts toward the budget, and more
    // workers mostly add CPU contention to every test without a faster run.
    maxWorkers: 2,
    setupFiles: ["src/test-setup.ts"],
    // Hard per-test limit; budgets are in scripts/test-budgets.json.
    testTimeout: 1000,
    reporters: ["default", ["json", { outputFile: "test-results/vitest.json" }]],
  },
});
