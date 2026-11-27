import { svelte } from "@sveltejs/vite-plugin-svelte";
import type { Plugin } from "vite";
import { configDefaults, defineConfig } from "vitest/config";
import { BaseSequencer, type TestSpecification } from "vitest/node";

/**
 * Files run in the order of their paths, the same on every machine and run:
 * files that share modules (see `test.pool`) see the same state before them.
 */
class PathOrder extends BaseSequencer {
  override async sort(files: TestSpecification[]): Promise<TestSpecification[]> {
    return [...files].sort((a, b) => (a.moduleId < b.moduleId ? -1 : a.moduleId > b.moduleId ? 1 : 0));
  }
}

/** Test files that run each in modules of their own (see `test.pool` below). */
const ISOLATED = [
  // vi.mock replaces a module for the whole file.
  "src/terminal/Terminals.test.ts",
  "src/lib/transport.tauri.test.ts",
  // Key and command registries, filled as modules load.
  "src/keys/context-keys.test.ts",
  "src/settings/Settings.test.ts",
  // Caches and remembered state of the whole window.
  "src/palette/files.test.ts",
  "src/palette/Palette.test.ts",
  "src/editor/open-files.test.ts",
  "src/editor/groups.test.ts",
  "src/agents/FirstSteps.svelte.test.ts",
  "src/agents/FirstSteps.ended.svelte.test.ts",
];

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

// Component tests compile components without the compiler's dev
// instrumentation (source locations, ownership and binding warnings), as the
// shipped build does; Svelte's runtime still runs its dev checks. Only in
// tests, templates are built with DOM calls instead of innerHTML (the shipped
// build keeps innerHTML), which jsdom would parse anew in every test file. The DOM is the same either way; both cut about a
// sixth of the suite's CPU time.
const testCompilerOptions = { dev: false, fragments: "tree" } as const;

export default defineConfig({
  plugins: [svelte(process.env.VITEST ? { compilerOptions: testCompilerOptions } : {}), xtermUnderFrozenPrototype()],
  // Pre-bundled dependencies skip plugin transforms, so the dev server would
  // serve xterm unfixed.
  optimizeDeps: { exclude: ["@xterm/xterm"] },
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  // Two pages: the desktop app and the phone app (`mobile.html`, served by
  // the phone listener, ADR 0014). They share chunks only where both import.
  build: { target: "es2022", rollupOptions: { input: { main: "index.html", mobile: "mobile.html" } } },
  // Vitest runs in Node; without the browser condition Svelte resolves to its
  // server build and component tests cannot call mount().
  resolve: process.env.VITEST ? { conditions: ["browser"] } : undefined,
  test: {
    environment: "jsdom",
    // One worker, and the files share one jsdom and one set of modules: about a
    // quarter less CPU time than a module context per file (pool vmThreads),
    // whose global lookups are slow, at the same wall time. Each test's CPU
    // time counts toward the budget, and a second worker only adds contention.
    // A file that needs modules of its own (vi.mock) or checks state a module
    // keeps for the whole window (registries, caches, remembered hints) goes in
    // ISOLATED; a test that passes alone but fails in the full run belongs there.
    pool: "threads",
    isolate: false,
    maxWorkers: 1,
    sequence: { sequencer: PathOrder },
    projects: [
      { extends: true, test: { name: "shared", exclude: [...configDefaults.exclude, ...ISOLATED] } },
      { extends: true, test: { name: "isolated", include: ISOLATED, pool: "vmThreads", isolate: true } },
    ],
    setupFiles: ["src/test-setup.ts"],
    runner: "src/test-runner.ts",
    // Hard per-test limit; budgets are in scripts/test-budgets.json.
    testTimeout: 1000,
    reporters: ["default", ["json", { outputFile: "test-results/vitest.json" }]],
  },
});
