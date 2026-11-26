import { describe, expect, it } from "vitest";
import icons from "./data";
import { iconName, iconUrl, loadIcons } from "./icons.svelte";

describe("iconName", () => {
  const file = (name: string) => iconName(icons, name, "file", false);
  const dir = (name: string, open = false) => iconName(icons, name, "dir", open);

  it("prefers the exact file name, then the longest known extension", () => {
    expect(file("package.json")).toBe("nodejs");
    expect(file("Cargo.toml")).toBe("toml");
    expect(file("a.d.ts")).toBe("typescript-def");
    expect(file("main.RS")).toBe("rust");
    expect(file("README.md")).toBe("readme");
    expect(file("VERSION")).toBe("file");
    expect(file(".DS_Store")).toBe("file");
    // Inherited object keys are not names.
    expect(file("constructor")).toBe("file");
  });

  it("finds folders by name, case and leading dots or underscores aside", () => {
    expect(dir(".claude")).toBe("folder-claude");
    expect(dir("Docs")).toBe("folder-docs");
    expect(dir("__tests__")).toBe("folder-test");
    expect(dir("Configs", true)).toBe("folder-config-open");
    expect(dir("openspec")).toBe("folder");
  });
});

describe("iconUrl", () => {
  it("is null until the table loads, then a data URL", async () => {
    await loadIcons();
    expect(iconUrl("a.rs", "file", false)).toMatch(/^data:image\/svg\+xml;charset=utf-8,%3Csvg/);
  });
});
