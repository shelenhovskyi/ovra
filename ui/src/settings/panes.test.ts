import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, it, vi } from "vitest";
import { editorPrefs } from "../editor/prefs.svelte";
import { registerCommand } from "../keys/commands";
import type { AgentSetup, Request } from "../lib/protocol";
import type { Transport } from "../lib/transport";
import { Workspace } from "../shell/workspace.svelte";
import { openSettings } from "./commands";
import { findPanes, PANES, type PaneId } from "./panes.svelte";
import TestHost from "./TestHost.svelte";
import { settingsWindow } from "./window.svelte";

const READY: AgentSetup = {
  path: null,
  executable: "/usr/local/bin/claude",
  version: "2.1.0",
  state: { state: "ready" },
  billing: { kind: "subscription" },
  api_key_in_env: false,
};

let cleanup: (() => void)[] = [];

afterEach(() => {
  settingsWindow.close(false);
  for (const off of cleanup) off();
  cleanup = [];
});

it("first opens on Agents, not on the project's long Trust pane", async () => {
  vi.resetModules();
  const { settings, PANES: fresh } = await import("./panes.svelte");
  expect(fresh.find((pane) => pane.id === settings.pane)?.title).toBe("Agents");
});

async function show(pane: PaneId, answers: Partial<Record<Request["method"], unknown>>, trusted = false) {
  const transport: Transport = {
    call: ((request: Request) =>
      request.method in answers
        ? Promise.resolve({ type: request.method, data: answers[request.method] })
        : Promise.reject(new Error(`unexpected ${request.method}`))) as Transport["call"],
    onEvent: () => () => {},
  };
  const target = document.createElement("div");
  document.body.append(target);
  const workspace = new Workspace();
  workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted };
  const app = mount(TestHost, { target, props: { transport, workspace } });
  cleanup.push(() => {
    unmount(app);
    target.remove();
  });
  openSettings(pane);
  for (let i = 0; i < 4; i++) {
    flushSync();
    await new Promise((resolve) => setImmediate(resolve));
  }
  flushSync();
  return target.querySelector<HTMLElement>("section.pane");
}

const ACCOUNTS = {
  accounts: [{ account: { provider: "claude", id: "default", name: "Personal", config_dir: null }, setup: READY }],
  found: [],
  project_account: null,
};

it("shows a ready Claude as plain facts, with no repair form until something is wrong", async () => {
  const pane = await show("claude", { "agents.setup": READY, "agents.accounts": ACCOUNTS });
  const text = pane?.textContent ?? "";
  expect(text).toContain("Claude 2.1.0 is ready");
  expect(text).toContain("Usage counts against your Claude plan's limits.");
  expect(pane?.querySelector("[data-testid=claude-executable]")?.textContent).toBe("/usr/local/bin/claude");
  expect(text).toContain("found automatically");
  // NEW-24: nothing to fix, so no "How to fix", no filled Check again and no path field.
  expect(text).not.toContain("How to fix");
  expect(pane?.querySelector("input#claude-pane-path")).toBeNull();
  expect(pane?.querySelector("button.primary")).toBeNull();
  [...(pane?.querySelectorAll("button") ?? [])].find((b) => b.textContent === "Set path…")?.click();
  flushSync();
  expect(pane?.querySelector("input#claude-pane-path")).not.toBeNull();
});

it("shows the setup card's repair steps when Claude is signed out", async () => {
  const pane = await show("claude", {
    "agents.setup": { ...READY, state: { state: "signed_out" } },
    "agents.accounts": ACCOUNTS,
  });
  expect(pane?.querySelector("[data-testid=claude-setup]")?.textContent).toContain("Sign in to Claude");
  expect(pane?.querySelector("[data-testid=claude-status] > dl")).toBeNull();
});

it("lists what trusting the project would load and leaves trusting to the trust card", async () => {
  let asked = 0;
  cleanup.push(registerCommand({ id: "projects.review_trust", title: "Review", run: () => void asked++ }));
  const config = {
    fingerprint: "f1",
    items: [{ path: ".claude/settings.json", kind: "hooks", entries: ["npm run lint"] }],
  };
  const pane = await show("trust", { "agents.project_config": { current: config, trusted: null } });
  expect(pane?.querySelector("[data-testid=trust-state]")?.textContent).toMatch(/^Not trusted/);
  expect(pane?.querySelector("section h4")?.textContent).toBe("Hooks: commands that run on agent events");
  expect(pane?.querySelector("li")?.textContent).toBe("npm run lint");
  [...(pane?.querySelectorAll("button") ?? [])].find((b) => b.textContent === "Review and trust…")?.click();
  await new Promise((resolve) => setImmediate(resolve));
  expect(asked).toBe(1);
});

it("edits the editor preferences, keeping numbers within their limits", async () => {
  const pane = await show("editor", { "info.get": { name: "ovra", version: "1", mode: "headless" } });
  const field = <T extends HTMLElement>(id: string) => pane?.querySelector<T>(`[data-testid=${id}]`) as T;
  const tab = field<HTMLInputElement>("editor-tab-size");
  tab.value = "40";
  tab.dispatchEvent(new Event("change", { bubbles: true }));
  const autoSave = field<HTMLSelectElement>("editor-auto-save");
  autoSave.value = "after_delay";
  autoSave.dispatchEvent(new Event("change"));
  field<HTMLInputElement>("editor-wrap").click();
  field<HTMLInputElement>("editor-spaces").click();
  flushSync();
  const prefs = [editorPrefs.tabSize, editorPrefs.autoSave, editorPrefs.wordWrap, editorPrefs.insertSpaces];
  expect([tab.value, ...prefs]).toEqual(["16", 16, "after_delay", true, false]);
  Object.assign(editorPrefs, { tabSize: 4, autoSave: "off", wordWrap: false, insertSpaces: true });
});

it("finds panes by every word of a search, titles that start with it first", () => {
  const ids = (query: string) => findPanes(PANES, query).map((pane) => pane.id);
  expect(ids("")).toHaveLength(PANES.length);
  expect(ids("key")).toEqual(["keys", "keyboard", "machines"]);
  expect(ids("api key")).toEqual(["keys"]);
  expect(ids("nothing like this")).toEqual([]);
});
