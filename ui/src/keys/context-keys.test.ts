import { expect, it } from "vitest";
import { listContextKeys, registerContextKeys } from "./context-keys";

it("lists declared keys in order and takes them back", () => {
  const offGit = registerContextKeys([
    { keys: "S", title: "Stage", where: "Git" },
    { keys: "U", title: "Unstage", where: "Git" },
  ]);
  const offTree = registerContextKeys([{ keys: "F2", title: "Rename", where: "Files" }]);
  expect(listContextKeys().map((k) => k.keys)).toEqual(["S", "U", "F2"]);
  offGit();
  expect(listContextKeys().map((k) => k.title)).toEqual(["Rename"]);
  offTree();
  expect(listContextKeys()).toEqual([]);
});
