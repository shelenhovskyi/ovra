// Contrast of the theme tokens that carry text (WCAG 2 ratios), so a colour
// change cannot quietly drop below the spec's 4.5:1 (docs/specs/quiet-style.md).
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

// Read from disk: Vitest does not load CSS, not even with `?raw`.
const css = readFileSync(`${import.meta.dirname}/theme.css`, "utf8");

type Rgba = [number, number, number, number];

function tokens(block: string): Record<string, string> {
  return Object.fromEntries([...block.matchAll(/--([\w-]+):\s*([^;]+);/g)].map((m) => [m[1], m[2]?.trim()]));
}

const lightAt = css.indexOf("@media (prefers-color-scheme: light)");
const dark = tokens(css.slice(0, lightAt));
const light = { ...dark, ...tokens(css.slice(lightAt, css.indexOf("\n:root {", lightAt))) };

function parse(value: string | undefined): Rgba {
  if (value?.startsWith("#")) return [1, 3, 5].map((i) => parseInt(value.slice(i, i + 2), 16)).concat(1) as Rgba;
  const rgba = value
    ?.match(/^rgba\(([^)]+)\)$/)?.[1]
    ?.split(",")
    .map(Number);
  if (rgba?.length !== 4) throw new Error(`not a colour: ${value}`);
  return rgba as Rgba;
}

const channel = (c: number) => (c / 255 <= 0.03928 ? c / 255 / 12.92 : ((c / 255 + 0.055) / 1.055) ** 2.4);
const luminance = ([r, g, b]: Rgba) => 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
const mix = (top: number, under: number, alpha: number) => top * alpha + under * (1 - alpha);
const over = ([r, g, b, a]: Rgba, [R, G, B]: Rgba): Rgba => [mix(r, R, a), mix(g, G, a), mix(b, B, a), 1];

function ratio(a: Rgba, b: Rgba): number {
  const [x, y] = [luminance(a), luminance(b)];
  return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05);
}

const SURFACES = ["s0", "s1", "s2", "s3", "s4"];
const TEXT = ["text", "text-2", "accent", "warn", "ok", "danger"];

// Surface steps: light needs wider ones than dark to read as separate areas.
describe.each([
  ["dark", dark, 1.04],
  ["light", light, 1.08],
] as const)("%s theme", (_, theme, step) => {
  const c = (name: string) => parse(theme[name]);
  const low: string[] = [];
  const check = (what: string, value: number, min: number) => {
    if (value < min) low.push(`${what} ${value.toFixed(2)} < ${min}`);
  };

  it("keeps text, status colours and comments at 4.5:1", () => {
    for (const text of TEXT) for (const s of SURFACES) check(`${text} on ${s}`, ratio(c(text), c(s)), 4.5);
    for (const tone of ["accent", "warn", "ok", "danger"])
      for (const s of ["s1", "s2"])
        check(`${tone} on ${tone}-soft/${s}`, ratio(c(tone), over(c(`${tone}-soft`), c(s))), 4.5);
    check("accent-ink on accent", ratio(c("accent-ink"), c("accent")), 4.5);
    for (const s of ["s1", "s2"]) check(`com on ${s}`, ratio(c("com"), c(s)), 4.5);
    for (const s of ["s0", "s1", "s2"]) check(`text-3 on ${s}`, ratio(c("text-3"), c(s)), 3);
    expect(low).toEqual([]);
  });

  it("tells the rail, panels and editor apart by tone", () => {
    check("s0 to s1", ratio(c("s0"), c("s1")), step);
    check("s1 to s2", ratio(c("s1"), c("s2")), step);
    expect(low).toEqual([]);
  });
});
