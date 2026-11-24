import { describe, expect, it } from "vitest";
import { resolveLink, slug } from "./links";

describe("resolveLink", () => {
  it("resolves relative and root-relative file links", () => {
    expect(resolveLink("docs/adr/README.md", "0001-x.md")).toEqual({ kind: "file", path: "docs/adr/0001-x.md" });
    expect(resolveLink("docs/adr/README.md", "../glossary.md#core")).toEqual({
      kind: "file",
      path: "docs/glossary.md",
    });
    expect(resolveLink("docs/a.md", "./img/my%20pic.png")).toEqual({ kind: "file", path: "docs/img/my pic.png" });
    expect(resolveLink("docs/a.md", "/CLAUDE.md")).toEqual({ kind: "file", path: "CLAUDE.md" });
  });

  it("keeps anchors in the document", () => {
    expect(resolveLink("a.md", "#rules")).toEqual({ kind: "anchor", id: "rules" });
  });

  it("does not navigate to other schemes or outside the project", () => {
    expect(resolveLink("a.md", "https://example.com")).toEqual({ kind: "external" });
    expect(resolveLink("a.md", "mailto:x@y.z")).toEqual({ kind: "external" });
    expect(resolveLink("a.md", "//evil.test/x")).toEqual({ kind: "external" });
    expect(resolveLink("docs/a.md", "../../etc/passwd")).toEqual({ kind: "external" });
    expect(resolveLink("a.md", "?q=1")).toEqual({ kind: "external" });
  });
});

describe("slug", () => {
  it("matches GitHub heading anchors", () => {
    expect(slug("Layout and boundaries")).toBe("layout-and-boundaries");
    expect(slug(" When to ask? ")).toBe("when-to-ask");
    expect(slug("Phase 1 — viewer")).toBe("phase-1--viewer");
  });
});
