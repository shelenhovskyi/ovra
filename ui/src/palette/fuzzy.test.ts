import { describe, expect, it } from "vitest";
import { fuzzyMatch, rank, segments } from "./fuzzy";

const titles = (items: { item: string }[]) => items.map((r) => r.item);

describe("fuzzy matching", () => {
  it("matches characters in order, ignoring case and spaces in the query", () => {
    expect(fuzzyMatch("sac", "Show all commands")?.positions).toEqual([0, 5, 9]);
    expect(fuzzyMatch("show all", "Show all commands")).not.toBeNull();
    expect(fuzzyMatch("lls", "Show all")).toBeNull();
    expect(fuzzyMatch("", "anything")).toEqual({ score: 0, positions: [] });
  });

  it("prefers word starts and runs over scattered letters", () => {
    expect(fuzzyMatch("of", "Open File")?.positions).toEqual([0, 5]);
    expect(fuzzyMatch("main", "src/domain/main.rs")?.positions).toEqual([11, 12, 13, 14]);
    const open = fuzzyMatch("of", "Open File")?.score ?? 0;
    const toggle = fuzzyMatch("of", "Toggle Profile")?.score ?? 0;
    expect(open).toBeGreaterThan(toggle);
  });

  it("ranks best first, shorter on ties, keeps order for an empty query and honours the limit", () => {
    const items = ["Toggle profile", "Open file", "Close"];
    expect(titles(rank(items, (s) => s, "of"))).toEqual(["Open file", "Toggle profile"]);
    expect(titles(rank(items, (s) => s, ""))).toEqual(items);
    expect(titles(rank(["src/a/lib.rs", "lib.rs"], (s) => s, "lib"))).toEqual(["lib.rs", "src/a/lib.rs"]);
    expect(rank(items, (s) => s, "o", 1)).toHaveLength(1);
  });

  it("ranks 20,000 paths per keystroke quickly", () => {
    const paths = Array.from({ length: 20_000 }, (_, i) => `crates/module-${i % 50}/src/file_${i}.rs`);
    const start = performance.now();
    const top = rank(paths, (s) => s, "mod7file", 100);
    expect(performance.now() - start).toBeLessThan(150);
    expect(top[0]?.item).toMatch(/^crates\/module-7\/src\/file_/);
  });

  it("splits a label into highlighted runs", () => {
    expect(segments("Open", [0, 1, 3])).toEqual([
      { text: "Op", hit: true },
      { text: "e", hit: false },
      { text: "n", hit: true },
    ]);
  });
});
