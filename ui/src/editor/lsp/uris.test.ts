import { expect, test } from "vitest";
import { pathOf, uriOf } from "./uris";

test("a project path becomes an ovra URI with each segment encoded, and back", () => {
  expect(uriOf("src/a b.rs")).toBe("ovra:///src/a%20b.rs");
  expect(uriOf("x/#1?/%.ts")).toBe("ovra:///x/%231%3F/%25.ts");
  expect(pathOf("ovra:///src/a%20b.rs")).toBe("src/a b.rs");
  for (const path of ["x/#1?/%.ts", "ü/日本.py", "a/b/c.json"]) expect(pathOf(uriOf(path))).toBe(path);
});

test("URIs outside the project have no path", () => {
  expect(pathOf("file:///usr/lib/rustlib/src/lib.rs")).toBeNull();
  expect(pathOf("ovra:///")).toBeNull();
  expect(pathOf("ovra:///bad%E0%A4%A")).toBeNull();
  expect(pathOf("ovra://host/a.rs")).toBeNull();
});

test("a server URI that leaves the project or hides a separator names no path", () => {
  for (const uri of [
    "ovra:///..%2F..%2F.ssh%2Fid_rsa",
    "ovra:///../x",
    "ovra:///a//b",
    "ovra:///a/./b",
    "ovra:///a%5Cb",
  ]) {
    expect(pathOf(uri), uri).toBeNull();
  }
});
