import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { installKeybindings } from "../keys/keybindings";
import type { DevServerInfo as ServerInfo } from "../lib/protocol";
import { mainView } from "../shell/main-view.svelte";
import { Workspace } from "../shell/workspace.svelte";
import { serversDock } from "./dock.svelte";
import { FakeServersCore } from "./fake-servers";
import TestHost from "./TestHost.svelte";

let app: Record<string, unknown> | null = null;
let target: HTMLElement;

beforeEach(() => {
  target = document.createElement("div");
  document.body.append(target);
});

afterEach(() => {
  serversDock.shown = false;
  serversDock.everywhere = true;
  serversDock.workDir = null;
  mainView.close(false);
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

/** The workspace of the panel `open` mounted last. */
let shown: Workspace | null = null;

async function open(core = new FakeServersCore()) {
  const workspace = new Workspace();
  shown = workspace;
  workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: false };
  app = mount(TestHost, { target, props: { transport: core, workspace } });
  serversDock.shown = true;
  await settle();
  return core;
}

const $ = <T extends HTMLElement>(selector: string) => target.querySelector<T>(selector);
const button = (name: string) =>
  [...target.querySelectorAll<HTMLButtonElement>("button")].find(
    (b) => b.getAttribute("aria-label") === name || b.textContent?.trim() === name,
  );
const dock = () => $<HTMLElement>("[data-testid=servers]");
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

describe("Servers pane and dock tab", () => {
  it("starts a defined server, shows it in the dock and follows its status", async () => {
    const core = await open();
    serversDock.shown = false;
    expect($("[data-testid=server-def]")?.textContent).toContain("pnpm dev");
    button("Start web")?.click();
    await settle();
    expect(core.calls).toContainEqual({
      method: "servers.start",
      params: { project: "p1", name: "web", work_dir: null },
    });
    expect(statuses()).toEqual(["starting"]);
    expect(dock()?.hidden).toBe(false);
    // While it runs, the definition's button shows it instead of starting another.
    expect(button("Start web")).toBeUndefined();
    expect(button("Show web")).toBeDefined();
    for (const status of [{ state: "ready" }, { state: "exited", code: 1 }] as const) {
      core.emit({ type: "servers.status", data: { id: "srv1", project: "p1", status, generation: 1 } });
      await settle();
    }
    // A failed start says so in words, here and in the dock, which shows its output with the reason.
    expect(statuses()).toEqual(["stopped: the command failed"]);
    expect($("[data-testid=server-status]")?.getAttribute("title")).toBe("Exit code 1");
    expect($("[data-testid=server-failed]")?.textContent).toBe("stopped: the command failed");
    expect($("[data-testid=server-log]")).not.toBeNull();
    expect(button("Start web")).toBeDefined();
    expect(button("Stop web")?.disabled).toBe(true);
  });

  it("keeps an Add server button under the list, and says No sandbox in the session's words", async () => {
    const core = new FakeServersCore();
    core.sessions = [
      {
        id: "s1",
        project: "p1",
        agent: "claude",
        mode: "direct",
        status: { state: "idle" },
        title: "fix it",
        usage: { input_tokens: 0, output_tokens: 0, cost_micro_usd: null },
        resumable: false,
        billing: null,
        sandbox: null,
        sandbox_reported: true,
        account: null,
      },
    ];
    await open(core);
    expect($("[data-testid=servers-unsandboxed]")?.textContent).toBe("No sandbox: Claude can change anything you can.");
    expect($("[data-testid=servers-add]")).toBeNull();
    button("Add server…")?.click();
    await settle();
    expect(document.activeElement).toBe($("[data-field=name]"));
    const fill = (field: string, text: string) => {
      const input = $<HTMLInputElement>(`[data-field=${field}]`);
      if (input) input.value = text;
      input?.dispatchEvent(new Event("input", { bubbles: true }));
    };
    fill("name", "api");
    fill("command", "cargo run");
    fill("port", "8080");
    $<HTMLFormElement>("[data-testid=servers-add]")?.requestSubmit();
    await settle();
    expect(core.calls.some((c) => c.method === "fs.write_file")).toBe(true);
    expect($("[data-testid=servers-add]")).toBeNull();
  });

  it("shows who started a server, where, its port and run, and stops and restarts it", async () => {
    const core = new FakeServersCore();
    core.servers = [server({ started_by: { kind: "agent", session: "s1" }, generation: 2 })];
    await open(core);
    const row = $("[data-testid=server]")?.textContent ?? "";
    expect(row).toContain("port 5173");
    expect(row).toContain("in project folder");
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

  it("adds a first server from the empty state's form", async () => {
    const core = new FakeServersCore();
    core.definitions = [];
    await open(core);
    expect($("[data-testid=servers-work-dir]")).toBeNull();
    expect($("[data-testid=servers-empty] details pre")?.textContent).toContain('"command": ["pnpm", "dev"]');
    const fill = (field: string, text: string) => {
      const input = $<HTMLInputElement>(`[data-field=${field}]`);
      if (input) input.value = text;
      input?.dispatchEvent(new Event("input", { bubbles: true }));
    };
    fill("name", "Web");
    $<HTMLFormElement>("[data-testid=servers-add]")?.requestSubmit();
    await settle();
    expect($("[data-testid=servers-add-error]")?.textContent).toMatch(/^Name it with lowercase letters/);
    expect(document.activeElement).toBe($("[data-field=name]"));
    expect(core.calls.some((c) => c.method === "fs.write_file")).toBe(false);

    fill("name", "api");
    fill("command", " cargo  run ");
    fill("folder", "server/");
    fill("port", "8080");
    $<HTMLFormElement>("[data-testid=servers-add]")?.requestSubmit();
    await settle();
    expect(core.calls).toContainEqual({
      method: "fs.write_file",
      params: {
        project: "p1",
        path: ".ovra/servers.json",
        text: `${JSON.stringify({ servers: [{ name: "api", command: ["cargo", "run"], port: 8080, cwd: "server" }] }, null, 2)}\n`,
        expected_mtime_ms: null,
      },
    });
    expect($("[data-testid=servers-empty]")).toBeNull();
    expect(document.activeElement).toBe(button("Start api"));
  });

  it("opens the servers file in the editor, starting from the example", async () => {
    const core = new FakeServersCore();
    core.definitions = [];
    await open(core);
    const workspace = shown;
    button("Open in the editor")?.click();
    await settle();
    expect(core.files.get(".ovra/servers.json")?.text).toContain('"name": "web"');
    expect([workspace?.openPath, workspace?.mode]).toEqual([".ovra/servers.json", "edit"]);
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
    expect($("[data-testid=servers-settings] [data-testid=servers-unavailable]")?.textContent).toContain(
      "need the Linux sandbox",
    );
    expect($("[data-testid=server-def]")).toBeNull();
  });

  it("lists every folder's servers, or one folder's", async () => {
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
        sandbox_reported: false,
        account: null,
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
    // At first the tab shows every folder's servers, each saying where it runs.
    expect(core.calls).toContainEqual({ method: "servers.overview", params: { project: "p1" } });
    expect($("[data-testid=server]")?.textContent).toContain("in Worker docs");
    const select = $<HTMLSelectElement>("[data-testid=servers] [data-testid=servers-work-dir]");
    const options = [...(select?.options ?? [])].map((o) => o.textContent);
    expect(options).toEqual(["All folders", "Project folder", "Worker docs"]);
    const pick = async (index: number) => {
      if (select) select.selectedIndex = index;
      select?.dispatchEvent(new Event("change", { bubbles: true }));
      await settle();
    };
    await pick(1);
    expect(core.calls).toContainEqual({ method: "servers.list", params: { project: "p1", work_dir: null } });
    expect($("[data-testid=server]")).toBeNull();
    await pick(2);
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
    expect(vite?.style.color).toBe("var(--ok)");
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

  it("opens with Mod+Alt+S on any keyboard layout", async () => {
    const workspace = new Workspace();
    workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: false };
    app = mount(TestHost, { target, props: { transport: new FakeServersCore(), workspace } });
    const off = installKeybindings(window, false, false);
    await settle();
    // A Cyrillic layout: the key is a Cyrillic letter, the physical key is KeyS.
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "\u044b", code: "KeyS", ctrlKey: true, altKey: true }));
    await settle();
    off();
    expect(dock()?.hidden).toBe(false);
    expect(document.activeElement).toBe($("[data-testid=servers] h2"));
  });

  it("says when nothing runs, with the way to the definitions", async () => {
    await open();
    expect(dock()?.textContent).toContain("No servers running");
    button("Server settings")?.click();
    expect(mainView.current).toBe("settings");
  });

  it("keeps focus on the server's buttons as Stop and Restart disable them, and hides on Escape", async () => {
    const core = new FakeServersCore();
    core.servers = [server()];
    await open(core);
    const press = (name: string) => {
      button(name)?.focus();
      button(name)?.click();
    };
    const status = async (state: "stopping" | "exited") => {
      const next = state === "exited" ? { state, code: null } : { state };
      core.emit({ type: "servers.status", data: { id: "srv1", project: "p1", status: next, generation: 1 } });
      await settle();
    };
    press("Stop web");
    await settle();
    await status("stopping");
    expect(document.activeElement?.textContent?.trim()).toBe("Output");
    await status("exited");
    expect(document.activeElement).toBe(button("Restart web"));
    press("Restart web");
    await settle();
    expect(document.activeElement).toBe(button("Restart web"));
    document.activeElement?.dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", bubbles: true }));
    await settle();
    expect(dock()?.hidden).toBe(true);
  });
});
