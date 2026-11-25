import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { ServerInfo } from "../lib/protocol";
import { Workspace } from "../shell/workspace.svelte";
import { FakeServersCore } from "./fake-servers";
import TestHost from "./TestHost.svelte";

let app: Record<string, unknown> | null = null;
let target: HTMLElement;

beforeEach(() => {
  target = document.createElement("div");
  document.body.append(target);
});

afterEach(() => {
  if (app) unmount(app);
  app = null;
  target.remove();
});

async function settle() {
  for (let i = 0; i < 16; i++) {
    flushSync();
    await Promise.resolve();
  }
  flushSync();
}

async function open(core = new FakeServersCore()) {
  const workspace = new Workspace();
  workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: false };
  app = mount(TestHost, { target, props: { transport: core, workspace } });
  await settle();
  $<HTMLButtonElement>("[data-testid=open-servers]")?.click();
  await settle();
  return core;
}

const $ = <T extends HTMLElement>(selector: string) => target.querySelector<T>(selector);
const button = (name: string) =>
  [...target.querySelectorAll<HTMLButtonElement>("button")].find(
    (b) => b.getAttribute("aria-label") === name || b.textContent?.trim() === name,
  );
const statuses = () => [...target.querySelectorAll("[data-testid=server-status]")].map((el) => el.textContent?.trim());

const server = (over: Partial<ServerInfo> = {}): ServerInfo => ({
  id: "srv1",
  project: "p1",
  name: "web",
  work_dir: null,
  status: { state: "ready" },
  generation: 1,
  port: 5173,
  started_by: { kind: "person" },
  ...over,
});

describe("ServersPanel", () => {
  it("starts a defined server and follows its status", async () => {
    const core = await open();
    expect($("[data-testid=server-def]")?.textContent).toContain("pnpm dev");
    button("Start web")?.click();
    await settle();
    expect(core.calls).toContainEqual({
      method: "servers.start",
      params: { project: "p1", name: "web", work_dir: null },
    });
    expect(statuses()).toEqual(["starting"]);
    expect(button("Start web")?.disabled).toBe(true);
    for (const status of [{ state: "ready" }, { state: "exited", code: 1 }] as const) {
      core.emit({ type: "servers.status", data: { id: "srv1", project: "p1", status, generation: 1 } });
      await settle();
    }
    expect(statuses()).toEqual(["exited (code 1)"]);
    expect(button("Start web")?.disabled).toBe(false);
    expect(button("Stop web")?.disabled).toBe(true);
  });

  it("shows who started a server, where, its port and run, and stops and restarts it", async () => {
    const core = new FakeServersCore();
    core.servers = [server({ started_by: { kind: "agent", session: "s1" }, generation: 2 })];
    await open(core);
    const row = $("[data-testid=server]")?.textContent ?? "";
    expect(row).toContain("port 5173");
    expect(row).toContain("in main checkout");
    expect(row).toContain("started by Session s1");
    expect(row).toContain("run 2");
    button("Stop web")?.click();
    await settle();
    expect(core.calls).toContainEqual({ method: "servers.stop", params: { id: "srv1" } });
    button("Restart web")?.click();
    await settle();
    expect(row).toContain("run 2");
    expect($("[data-testid=server]")?.textContent).toContain("run 3");
    expect(statuses()).toEqual(["starting"]);
  });

  it("explains an empty file with an example", async () => {
    const core = new FakeServersCore();
    core.definitions = [];
    await open(core);
    const empty = $("[data-testid=servers-empty]")?.textContent ?? "";
    expect(empty).toContain(".ovra/servers.json");
    expect(empty).toContain('"command": ["pnpm", "dev"]');
  });

  it("shows a broken file with its path", async () => {
    const core = new FakeServersCore();
    core.definitions = [];
    core.fileError = "line 3: expected `,`";
    await open(core);
    expect($("[data-testid=servers-file-error]")?.textContent).toContain(
      "Could not read .ovra/servers.json: line 3: expected `,`",
    );
    expect($("[data-testid=servers-empty]")).toBeNull();
  });

  it("says servers need the Linux sandbox when they are unavailable", async () => {
    const core = new FakeServersCore();
    core.refuse.set("servers.list", { code: "unavailable", message: "no sandbox on this system" });
    await open(core);
    expect($("[data-testid=servers-unavailable]")?.textContent).toContain("need the Linux sandbox");
    expect($("[data-testid=server-def]")).toBeNull();
  });

  it("lists servers of a worker's worktree", async () => {
    const core = new FakeServersCore();
    core.sessions = [
      {
        id: "w1",
        project: "p1",
        agent: "claude",
        mode: "direct",
        status: { state: "idle" },
        title: "docs",
        usage: { input_tokens: 0, output_tokens: 0, cost_micro_usd: null },
        resumable: false,
        billing: null,
        sandbox: null,
      },
    ];
    core.servers = [server({ work_dir: "w1" })];
    const orchestration = core.call.bind(core);
    core.call = (async (request) =>
      request.method === "orchestration.list"
        ? {
            type: "orchestration.list",
            data: [
              { id: "t1", coordinator: "c1", workers: [{ session: "w1", name: "docs", branch: "b", kept: null }] },
            ],
          }
        : orchestration(request)) as typeof core.call;
    await open(core);
    expect($("[data-testid=server]")).toBeNull();
    const select = $<HTMLSelectElement>("[data-testid=servers-work-dir]");
    expect([...(select?.options ?? [])].map((o) => o.textContent)).toEqual(["Main checkout", "Worker docs"]);
    if (select) select.selectedIndex = 1;
    select?.dispatchEvent(new Event("change", { bubbles: true }));
    await settle();
    expect(core.calls).toContainEqual({ method: "servers.list", params: { project: "p1", work_dir: "w1" } });
    expect($("[data-testid=server]")?.textContent).toContain("in Worker docs");
  });

  it("shows the kept output, then live lines, with colours as text", async () => {
    const core = new FakeServersCore();
    core.servers = [server()];
    core.logs.set("srv1", [
      { n: 1, text: "\x1b[32mVITE\x1b[0m starting" },
      { n: 2, text: "<b>not bold</b>" },
    ]);
    await open(core);
    button("Output")?.click();
    await settle();
    core.emit({
      type: "servers.output",
      data: {
        id: "srv1",
        lines: [
          { n: 2, text: "<b>not bold</b>" },
          { n: 3, text: "ready in 300 ms" },
        ],
      },
    });
    await settle();
    const lines = [...target.querySelectorAll("[data-testid=server-log] .line")];
    expect(lines.map((l) => l.textContent)).toEqual(["VITE starting", "<b>not bold</b>", "ready in 300 ms"]);
    const vite = lines[0]?.querySelector<HTMLElement>("span");
    expect(vite?.textContent).toBe("VITE");
    expect(vite?.style.color).toBe("rgb(126, 231, 135)");
    expect(target.querySelector("[data-testid=server-log] b")).toBeNull();
  });

  it("follows new output until the person scrolls up", async () => {
    const core = new FakeServersCore();
    core.servers = [server()];
    await open(core);
    button("Output")?.click();
    await settle();
    const box = $<HTMLElement>("[role=log]");
    if (!box) throw new Error("no log");
    Object.defineProperty(box, "scrollHeight", { configurable: true, get: () => 1000 });
    Object.defineProperty(box, "clientHeight", { configurable: true, get: () => 200 });
    core.emit({ type: "servers.output", data: { id: "srv1", lines: [{ n: 1, text: "one" }] } });
    await settle();
    expect(box.scrollTop).toBe(1000);
    box.scrollTop = 100;
    box.dispatchEvent(new Event("scroll"));
    await settle();
    core.emit({ type: "servers.output", data: { id: "srv1", lines: [{ n: 2, text: "two" }] } });
    await settle();
    expect(box.scrollTop).toBe(100);
    button("Jump to the newest line")?.click();
    box.dispatchEvent(new Event("scroll"));
    await settle();
    expect(box.scrollTop).toBe(1000);
    expect(button("Jump to the newest line")).toBeUndefined();
  });

  it("closes on Escape", async () => {
    await open();
    $("[data-testid=servers] h2")?.dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", bubbles: true }));
    await settle();
    expect($("[data-testid=servers]")).toBeNull();
  });
});
