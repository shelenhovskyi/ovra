import { expect, test } from "vitest";
import type { Transport } from "../lib/transport";
import { Workspace } from "../shell/workspace.svelte";
import { palette, showList } from "./mode.svelte";
import { source } from "./sources";

test("showList opens the palette on a list another area hands over", () => {
  const workspace = new Workspace();
  const list = { name: "Symbols", placeholder: "Go to symbol", empty: "None", limit: 50, entries: () => [] };
  showList(list, workspace);
  expect([palette.mode, workspace.paletteOpen]).toEqual(["list", true]);
  expect(source("list", {} as Transport, workspace).placeholder).toBe("Go to symbol");
  palette.list = null;
  expect(source("list", {} as Transport, workspace).entries()).toEqual([]);
  palette.mode = "commands";
});
