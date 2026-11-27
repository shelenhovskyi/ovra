import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, it } from "vitest";
import type { CatalogItem, Request, ServerUse } from "../lib/protocol";
import type { Transport } from "../lib/transport";
import ServerSwitch from "./ServerSwitch.svelte";

const ITEM: CatalogItem = {
  kind: "mcp_server",
  name: "db",
  description: "npx db-mcp",
  path: ".mcp.json",
  agents: ["claude"],
  loads: "not_in_ovra",
  fields: [],
  files: [],
  problem: null,
};
const OFF: ServerUse = { on: false, changed: false, blocked: null, host: null, tools: [] };

let cleanup: (() => void)[] = [];

afterEach(() => {
  for (const off of cleanup) off();
  cleanup = [];
});

async function settle(): Promise<void> {
  for (let i = 0; i < 3; i++) {
    flushSync();
    await new Promise((resolve) => setImmediate(resolve));
  }
}

function show(use: ServerUse) {
  const calls: Request[] = [];
  let reloads = 0;
  const transport: Transport = {
    call: ((request: Request) => {
      calls.push(request);
      return Promise.resolve({ type: "ok" });
    }) as Transport["call"],
    onEvent: () => () => {},
  };
  const target = document.createElement("div");
  document.body.append(target);
  const app = mount(ServerSwitch, {
    target,
    props: { transport, project: "p1", item: ITEM, use, changed: () => reloads++ },
  });
  cleanup.push(() => {
    void unmount(app);
    target.remove();
  });
  const $ = <T extends Element>(id: string) => target.querySelector<T>(`[data-testid=${id}]`);
  return { target, calls, reloads: () => reloads, $ };
}

it("turns a server on only after saying what it runs, and off at once", async () => {
  const off = show(OFF);
  off.$<HTMLInputElement>("server-switch")?.click();
  flushSync();
  expect(off.calls).toEqual([]);
  expect(off.$("server-confirm")?.textContent).toContain("npx db-mcp");
  off.$<HTMLButtonElement>("server-turn-on")?.click();
  await settle();
  expect(off.calls).toEqual([{ method: "catalog.set_server", params: { project: "p1", name: "db", on: true } }]);
  expect(off.reloads()).toBe(1);

  const on = show({ ...OFF, on: true, host: "docs.example", tools: [{ name: "drop", on: true }] });
  const tool = on.target.querySelector<HTMLInputElement>(".tools input");
  tool?.click();
  await settle();
  on.$<HTMLInputElement>("server-switch")?.click();
  await settle();
  expect(on.calls.map((c) => ("params" in c ? c.params : null))).toEqual([
    { project: "p1", server: "db", tool: "drop", on: false },
    { project: "p1", name: "db", on: false },
  ]);
});

it("says why a server cannot be used, and when the file changed it", () => {
  const blocked = show({ ...OFF, blocked: "Needs DB_TOKEN from your environment." });
  expect(blocked.$("server-blocked")?.textContent).toContain("DB_TOKEN");
  expect(blocked.$("server-switch")).toBeNull();
  const changed = show({ ...OFF, changed: true });
  expect(changed.$("server-changed")?.textContent).toContain("no longer get it");
  changed.$<HTMLInputElement>("server-switch")?.click();
  flushSync();
  expect(changed.$("server-confirm")).not.toBeNull();
});
