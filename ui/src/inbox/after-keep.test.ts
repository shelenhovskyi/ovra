import { describe, expect, it } from "vitest";
import { testsPrompt } from "./after-keep";

describe("testsPrompt", () => {
  it("names the task and each kept file", () => {
    const text = testsPrompt("Fix the parser", ["src/a.ts", "src/b.ts"]);
    expect(text.split("\n")[0]).toBe('Write tests for the changes I just kept from "Fix the parser".');
    expect(text).toContain("- src/a.ts\n- src/b.ts\n");
  });

  it("names at most thirty files", () => {
    const files = Array.from({ length: 32 }, (_, i) => `f${i}.ts`);
    const text = testsPrompt("t", files);
    expect(text).toContain("- f29.ts\n- and 2 more");
    expect(text).not.toContain("f30.ts");
  });
});
