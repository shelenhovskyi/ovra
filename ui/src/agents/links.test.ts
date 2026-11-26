import { expect, it } from "vitest";
import { chatLink } from "./links";

const root = "/home/p/app";

it("opens web pages, and project files only from the project's own checkout", () => {
  expect(chatLink("https://example.com/a", root, root)).toEqual({ kind: "web", url: "https://example.com/a" });
  expect(chatLink("src/main.rs:12", root, root)).toEqual({ kind: "file", path: "src/main.rs" });
  expect(chatLink(`${root}/src/lib.rs`, root, root)).toEqual({ kind: "file", path: "src/lib.rs" });
  expect(chatLink("src/main.rs", root, "/home/p/.ovra/wt/1")).toBeNull();
  expect(chatLink("/etc/passwd", root, root)).toBeNull();
  expect(chatLink("../outside.txt", root, root)).toBeNull();
  expect(chatLink("javascript:alert(1)", root, root)).toBeNull();
  expect(chatLink("file:///etc/passwd", root, root)).toBeNull();
});
