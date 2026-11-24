import { describe, expect, it } from "vitest";
import { pathScopes } from "./scope";

const inside = (path: string, root = "/work/demo") => pathScopes({ file_path: path }, root)[0]?.inside;

describe("pathScopes", () => {
  it("tells paths inside the project from paths outside it", () => {
    expect(inside("/work/demo/README.md")).toBe(true);
    expect(inside("src/main.rs")).toBe(true);
    expect(inside("/etc/passwd")).toBe(false);
    expect(inside("/work/demo-other/a")).toBe(false);
  });

  it("is not fooled by dots", () => {
    expect(inside("../demo-other/a")).toBe(false);
    expect(inside("/work/demo/../../etc/passwd")).toBe(false);
    expect(inside("./src/../README.md")).toBe(true);
  });

  it("compares Windows paths without case", () => {
    expect(inside("c:\\Work\\Demo\\a.txt", "C:\\work\\demo")).toBe(true);
    expect(inside("D:\\work\\demo\\a.txt", "C:\\work\\demo")).toBe(false);
  });

  it("lists every path key and ignores input without paths", () => {
    expect(pathScopes({ path: "/tmp", pattern: "*" }, "/work/demo")).toEqual([{ path: "/tmp", inside: false }]);
    expect(pathScopes({ command: "ls" }, "/work/demo")).toEqual([]);
  });
});
