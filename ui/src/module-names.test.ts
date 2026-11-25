import { expect, test } from "vitest";

// `x.svelte.ts` is imported as "./x.svelte". On a case-insensitive file system
// (macOS by default) that path also names a component `X.svelte` in the same
// folder, and the build takes the component instead.
test("no rune module is imported by a path that also names a component", () => {
  const components = new Set(Object.keys(import.meta.glob("./**/*.svelte")).map((p) => p.toLowerCase()));
  const clashes = Object.keys(import.meta.glob("./**/*.svelte.ts"))
    .map((p) => p.slice(0, -".ts".length))
    .filter((p) => components.has(p.toLowerCase()));
  expect(clashes).toEqual([]);
});
