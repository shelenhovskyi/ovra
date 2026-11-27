import { describe, expect, it } from "vitest";
import { splitLine, spotLabel } from "./line-suffix";

describe("splitLine", () => {
  it("reads a line, and a column from 1, after a colon", () => {
    expect(splitLine("main.rs:42")).toEqual({ text: "main.rs", spot: { line: 42, from: 0, to: 0 } });
    expect(splitLine("src/main.rs:42:7")).toEqual({ text: "src/main.rs", spot: { line: 42, from: 6, to: 6 } });
    expect(splitLine("main.rs:42:7: ")).toEqual({ text: "main.rs", spot: { line: 42, from: 6, to: 6 } });
  });

  it("reads the parenthesised form", () => {
    expect(splitLine("Main.cs(42)")).toEqual({ text: "Main.cs", spot: { line: 42, from: 0, to: 0 } });
    expect(splitLine("Main.cs(42,7)")).toEqual({ text: "Main.cs", spot: { line: 42, from: 6, to: 6 } });
  });

  it("takes the last colon as the suffix", () => {
    expect(splitLine("a:b:3")).toEqual({ text: "a:b", spot: { line: 3, from: 0, to: 0 } });
  });

  it("drops a suffix still being typed, without a spot", () => {
    expect(splitLine("main.rs:")).toEqual({ text: "main.rs", spot: null });
    expect(splitLine("main.rs:12:")).toEqual({ text: "main.rs", spot: { line: 12, from: 0, to: 0 } });
  });

  it("leaves a query without a suffix alone", () => {
    expect(splitLine("main")).toEqual({ text: "main", spot: null });
    expect(splitLine("main(")).toEqual({ text: "main(", spot: null });
    expect(splitLine("v1.2")).toEqual({ text: "v1.2", spot: null });
  });

  it("reads a bare suffix, for the shown file", () => {
    expect(splitLine(":42")).toEqual({ text: "", spot: { line: 42, from: 0, to: 0 } });
  });

  it("labels the spot as typed", () => {
    expect(spotLabel({ line: 42, from: 0, to: 0 })).toBe("Line 42");
    expect(spotLabel({ line: 42, from: 6, to: 6 })).toBe("Line 42, column 7");
  });
});
