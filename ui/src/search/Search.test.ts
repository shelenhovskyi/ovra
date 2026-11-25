import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, expect, it } from "vitest";
import { Workspace } from "../shell/workspace.svelte";
import { FakeSearchCore } from "./fake-search";
import { findInFolder } from "./find-in";
import TestHost from "./TestHost.svelte";

let app: Record<string, unknown> | null = null;
const target = document.body.appendChild(document.createElement("div"));

afterEach(() => {
  if (app) unmount(app);
  app = null;
});

it("puts the cursor in the search box when the Search view is chosen or a folder is searched, not at start", async () => {
  const workspace = new Workspace();
  workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: false };
  workspace.sidebar = "search";
  app = mount(TestHost, { target, props: { transport: new FakeSearchCore(), workspace } });
  flushSync();
  await tick();
  const input = target.querySelector<HTMLInputElement>("input[type=search]");
  expect(document.activeElement).not.toBe(input);

  workspace.sidebar = "files";
  flushSync();
  const tab = target.querySelector<HTMLButtonElement>("[data-testid=search-tab]");
  tab?.focus();
  tab?.click();
  flushSync();
  await tick();
  expect(document.activeElement).toBe(input);

  // "Find in folder" from the file tree: limited to the folder, cursor in the box.
  workspace.sidebar = "files";
  flushSync();
  (document.activeElement as HTMLElement).blur();
  findInFolder("src/ui");
  await tick();
  flushSync();
  const include = target.querySelector<HTMLInputElement>("input[placeholder='*.ts, src']");
  expect(workspace.sidebar).toBe("search");
  expect(include?.value).toBe("src/ui/**");
  expect(document.activeElement).toBe(input);
});
