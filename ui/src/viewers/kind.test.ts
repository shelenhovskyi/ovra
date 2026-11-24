import { describe, expect, it } from "vitest";
import { viewFor } from "./kind";

describe("viewFor", () => {
  it("routes text files by extension", () => {
    expect(viewFor("docs/README.MD", { kind: "text", text: "# hi" })).toEqual({ kind: "markdown", text: "# hi" });
    expect(viewFor("fix.patch", { kind: "text", text: "x" })).toEqual({ kind: "patch", text: "x" });
    expect(viewFor("a.diff", { kind: "text", text: "x" })).toEqual({ kind: "patch", text: "x" });
    expect(viewFor("src/.md", { kind: "text", text: "x" })).toEqual({ kind: "text", text: "x" });
    expect(viewFor("Makefile", { kind: "text", text: "x" })).toEqual({ kind: "text", text: "x" });
  });

  it("turns images into data URLs", () => {
    expect(viewFor("a.png", { kind: "binary", mime: "image/png", base64: "AAA=" })).toEqual({
      kind: "image",
      src: "data:image/png;base64,AAA=",
    });
    expect(viewFor("logo.svg", { kind: "text", text: '<svg a="1"/>' })).toEqual({
      kind: "image",
      src: "data:image/svg+xml;charset=utf-8,%3Csvg%20a%3D%221%22%2F%3E",
    });
  });

  it("does not render other binaries or large files", () => {
    expect(viewFor("a.bin", { kind: "binary", mime: "application/octet-stream", base64: "" })).toEqual({
      kind: "binary",
      mime: "application/octet-stream",
    });
    expect(viewFor("big.md", { kind: "too_large" })).toEqual({ kind: "too_large" });
  });
});
