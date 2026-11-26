import { expect, it } from "vitest";
import { chatLink } from "./links";

const root = "/home/p/app";

it("opens web pages, and project files only from the project's own checkout", () => {
  expect(chatLink("https://example.com/a", root, root)).toEqual({ kind: "web", url: "https://example.com/a" });
  expect(chatLink("src/main.rs:12", root, root)).toEqual({ kind: "file", path: "src/main.rs", line: 12 });
  expect(chatLink("main.rs:4", root, root)).toEqual({ kind: "file", path: "main.rs", line: 4 });
  expect(chatLink("src/main.rs#L7", root, root)).toEqual({ kind: "file", path: "src/main.rs", line: 7 });
  expect(chatLink(`${root}/src/lib.rs`, root, root)).toEqual({ kind: "file", path: "src/lib.rs", line: null });
  expect(chatLink("src/main.rs", root, "/home/p/.ovra/wt/1")).toBeNull();
  expect(chatLink("/etc/passwd", root, root)).toBeNull();
  expect(chatLink("../outside.txt", root, root)).toBeNull();
  expect(chatLink(`${root}/%2e%2e/x`, root, root)).toBeNull();
  expect(chatLink("javascript:alert(1)", root, root)).toBeNull();
  expect(chatLink("file:///etc/passwd", root, root)).toBeNull();
  expect(chatLink("src/%E0%A4%A.rs", root, root)).toBeNull();
  expect(chatLink("#intro", root, root)).toBeNull();
  expect(chatLink("src/a.rs#x", root, root)).toEqual({ kind: "file", path: "src/a.rs", line: null });
});

it("finds absolute links in a folder with spaces and non-Latin letters", () => {
  const spaced = "/home/p/My Proj \u00e9";
  const href = encodeURI(`${spaced}/a b/x.rs:3`);
  expect(chatLink(href, spaced, spaced)).toEqual({ kind: "file", path: "a b/x.rs", line: 3 });
});
