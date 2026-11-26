import { describe, expect, it } from "vitest";
import { itemPrompt, listItems, numbersText } from "./items";

describe("listItems", () => {
  it("takes the top-level items with their sub-points", () => {
    const text = [
      "Found three things:",
      "",
      "1. **Race** in `watch.rs`",
      "   - the tick reads stale state",
      "",
      "   Fix it under the lock.",
      "2. [Slow test](tests/a.rs) takes 2 s",
      "3) Typo in README",
      "",
      "Want me to start?",
    ].join("\n");
    expect(listItems(text)).toEqual([
      {
        n: 1,
        title: "Race in watch.rs",
        body: "**Race** in `watch.rs`\n- the tick reads stale state\n\nFix it under the lock.",
      },
      { n: 2, title: "Slow test takes 2 s", body: "[Slow test](tests/a.rs) takes 2 s" },
      { n: 3, title: "Typo in README", body: "Typo in README" },
    ]);
  });

  it("needs two items, ignores code blocks and picks the longest list", () => {
    expect(listItems("1. Only one")).toEqual([]);
    expect(listItems("```\n1. a\n2. b\n```")).toEqual([]);
    const two = "Steps:\n\n1. a\n2. b\n\nIdeas:\n\n1. x\n2. y\n3. z";
    expect(listItems(two).map((i) => i.title)).toEqual(["x", "y", "z"]);
    const nested = "- group\n  1. a\n  2. b";
    expect(listItems(nested)).toEqual([]);
  });

  it("keeps a code block inside an item as part of it", () => {
    const text = "1. Run this:\n   ```\n   2. not an item\n   ```\n2. Then that";
    expect(listItems(text).map((i) => i.n)).toEqual([1, 2]);
    expect(listItems(text)[0]?.body).toContain("2. not an item");
  });
});

describe("item prompts", () => {
  it("says the numbers and quotes the item", () => {
    expect(numbersText([2])).toBe("2");
    expect(numbersText([1, 3, 5])).toBe("1, 3 and 5");
    const prompt = itemPrompt("Find bugs\nin core", { n: 3, title: "Typo", body: "Typo\n- in README" }, 2);
    expect(prompt).toBe(
      'Item 3 of the answer to "Find bugs":\n\n> Typo\n> - in README\n\nDo this item. Other agents do 2 other items of that list at the same time, so change only what this one needs.',
    );
    expect(itemPrompt("", { n: 1, title: "a", body: "a" }, 0)).toContain("Change only what this item needs.");
  });
});
