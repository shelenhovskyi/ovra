import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, it } from "vitest";
import { agentPrefs } from "../agents/prefs.svelte";
import { listCommands, registerCommand } from "../keys/commands";
import { registerContextKeys } from "../keys/context-keys";
import type { ConfigItem, CoreEvent, ProjectConfigReview, Request, Response } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";
import { mainView } from "../shell/main-view.svelte";
import { Workspace } from "../shell/workspace.svelte";
import { registerSettingsCommands } from "./commands";
import TestHost from "./TestHost.svelte";

const transport: Transport = {
  call: ((request: Request) =>
    request.method === "info.get"
      ? Promise.resolve({ type: "info", data: { name: "ovra", version: "1.2.3", mode: "headless" } })
      : Promise.reject(new Error(`unexpected ${request.method}`))) as Transport["call"],
  onEvent: () => () => {},
};

let cleanup: (() => void)[] = [];

afterEach(() => {
  mainView.close(false);
  for (const off of cleanup) off();
  cleanup = [];
});

async function settle(): Promise<void> {
  flushSync();
  await new Promise((resolve) => setImmediate(resolve));
  flushSync();
}

it("opens on a pane from its command, moves with the arrow keys and reopens on the last pane", async () => {
  const target = document.createElement("div");
  document.body.append(target);
  const workspace = new Workspace();
  const app = mount(TestHost, { target, props: { transport, workspace } });
  cleanup.push(
    registerSettingsCommands(),
    registerCommand({ id: "t.save", title: "Save", keys: "Mod+S", run: () => {} }),
  );
  cleanup.push(() => {
    unmount(app);
    target.remove();
  });
  const run = (id: string) =>
    listCommands()
      .find((c) => c.id === id)
      ?.run();

  await run("settings.keyboard");
  await settle();
  const pane = () => target.querySelector("[data-testid=settings] section.pane");
  expect(pane()?.querySelector("h3")?.textContent).toBe("Keyboard");
  expect([...(pane()?.querySelectorAll("tr") ?? [])].map((row) => row.textContent)).toEqual([
    "Open settingsCtrl+,",
    "SaveCtrl+S",
  ]);
  // The frame's title takes focus; the list is one tab stop, and arrows move in it.
  expect(document.activeElement?.textContent).toBe("Settings");
  const current = () => target.querySelector<HTMLElement>("nav [aria-current=page]");
  current()?.dispatchEvent(new KeyboardEvent("keydown", { code: "ArrowDown", bubbles: true }));
  flushSync();
  expect(current()?.textContent).toBe("Accessibility");
  expect(document.activeElement).toBe(current());
  current()?.dispatchEvent(new KeyboardEvent("keydown", { code: "End", bubbles: true }));
  await settle();
  expect(pane()?.textContent).toContain("ovra 1.2.3");

  mainView.close(false);
  await run("settings.open");
  flushSync();
  expect(current()?.textContent).toBe("About");
  // The key that opened it closes it again.
  await run("settings.open");
  flushSync();
  expect(pane()).toBeNull();
});

it("lists every key by where it works, keys of one place too, and filters them", async () => {
  const target = document.body.appendChild(document.createElement("div"));
  const app = mount(TestHost, { target, props: { transport, workspace: new Workspace() } });
  cleanup.push(
    registerSettingsCommands(),
    registerContextKeys([
      { keys: "Y", title: "Allow once", where: "Approvals" },
      { keys: "S", title: "Stage", where: "Git" },
    ]),
    () => {
      unmount(app);
      target.remove();
    },
  );
  await listCommands()
    .find((c) => c.id === "settings.keyboard")
    ?.run();
  await settle();
  const groups = () =>
    [...target.querySelectorAll("table")].map((table) => [
      document.getElementById(table.getAttribute("aria-labelledby") ?? "")?.textContent,
      [...table.querySelectorAll("tr")].map((row) => row.textContent),
    ]);
  expect(groups()).toEqual([
    ["Everywhere", ["Open settingsCtrl+,"]],
    ["Approvals", ["Allow onceY"]],
    ["Git", ["StageS"]],
  ]);
  expect(target.querySelector("section.pane")?.textContent).toContain("Letters follow your Latin layout");

  const filter = target.querySelector<HTMLInputElement>("[data-testid=keys-filter]");
  if (!filter) throw new Error("no filter");
  filter.value = "git";
  filter.dispatchEvent(new Event("input", { bubbles: true }));
  flushSync();
  expect(groups()).toEqual([["Git", ["StageS"]]]);
  // The first Escape clears the filter and keeps Settings open.
  filter.dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", bubbles: true, cancelable: true }));
  flushSync();
  expect(filter.value).toBe("");
  expect(mainView.current).toBe("settings");
  expect(groups()).toHaveLength(3);
});

it("shows only the open project's Claude settings, and re-reads them when they may have changed", async () => {
  const asked: { project: string; answer: (review: ProjectConfigReview) => void }[] = [];
  const listeners: ((event: CoreEvent) => void)[] = [];
  const core: Transport = {
    call: ((request: Request) =>
      request.method === "agents.project_config"
        ? new Promise<Response>((resolve) =>
            asked.push({
              project: request.params.project,
              answer: (data) => resolve({ type: "agents.project_config", data }),
            }),
          )
        : transport.call(request)) as Transport["call"],
    onEvent: (listener) => {
      listeners.push(listener);
      return () => listeners.splice(listeners.indexOf(listener), 1);
    },
  };
  const emit = (event: CoreEvent) => {
    for (const listener of listeners) listener(event);
  };
  const config = (items: ConfigItem[]): ProjectConfigReview => ({
    current: { fingerprint: "f", items },
    trusted: null,
  });
  const skills = config([{ path: ".claude/skills", kind: "skills", entries: ["a"] }]);
  const project = (id: string) => ({ id, name: id, root: `/${id}`, last_opened_ms: null, trusted: false });
  const target = document.createElement("div");
  document.body.append(target);
  const workspace = new Workspace();
  workspace.project = project("p1");
  const app = mount(TestHost, { target, props: { transport: core, workspace } });
  cleanup.push(registerSettingsCommands(), () => {
    unmount(app);
    target.remove();
  });
  await listCommands()
    .find((c) => c.id === "settings.trust")
    ?.run();
  await settle();
  const pane = () => target.querySelector("[data-testid=trust-settings]")?.textContent ?? "";
  asked[0]?.answer(skills);
  await settle();
  expect(pane()).toContain("Not trusted");

  // "Review and trust…" unfolds a folded Agents column before it opens the trust card there.
  const ran: string[] = [];
  const column = document.createElement("div");
  column.dataset.testid = "agents";
  column.innerHTML = `<div class="strip"></div>`;
  document.body.append(column);
  cleanup.push(
    () => column.remove(),
    registerCommand({ id: "layout.toggle_agents", title: "Agents", run: () => void ran.push("unfold") }),
    registerCommand({ id: "projects.review_trust", title: "Trust", run: () => void ran.push("trust card") }),
  );
  [...target.querySelectorAll("button")].find((b) => b.textContent === "Review and trust…")?.click();
  await settle();
  expect(ran).toEqual(["unfold", "trust card"]);

  // A read for p1 still on its way when p2 opens: p1's list neither stays nor comes back.
  emit({ type: "fs.changed", data: { project: "p1", paths: [".claude/skills/a/SKILL.md"] } });
  workspace.project = project("p2");
  await settle();
  expect(pane()).toContain("Reading the project's settings");
  // The project panes name the project they change.
  expect(target.querySelector("nav h3")?.textContent).toBe("This project · p2");
  expect(asked.map((a) => a.project)).toEqual(["p1", "p1", "p2"]);
  asked[2]?.answer(config([]));
  asked[1]?.answer(skills);
  await settle();
  expect(pane()).toContain("None: this project brings no Claude settings");

  emit({ type: "review.changed", data: { project: "p1", id: "r1" } });
  emit({ type: "review.changed", data: { project: "p2", id: "r2" } });
  await settle();
  expect(asked.map((a) => a.project)).toEqual(["p1", "p1", "p2", "p2"]);
});

it("sets how many agents work at once and shows what the core kept", async () => {
  const sent: number[] = [];
  let limit = { limit: 3, default: 3, min: 1, max: 8 };
  const core: Transport = {
    call: ((request: Request) => {
      if (request.method === "agents.limit") return Promise.resolve({ type: "agents.limit", data: limit });
      if (request.method === "agents.set_limit") {
        sent.push(request.params.limit);
        if (request.params.limit > 8)
          return Promise.reject(new CoreError({ code: "bad_request", message: "the limit must be between 1 and 8" }));
        limit = { ...limit, limit: request.params.limit };
        return Promise.resolve({ type: "agents.limit", data: limit });
      }
      return transport.call(request);
    }) as Transport["call"],
    onEvent: () => () => {},
  };
  const target = document.createElement("div");
  document.body.append(target);
  const app = mount(TestHost, { target, props: { transport: core, workspace: new Workspace() } });
  cleanup.push(registerSettingsCommands(), () => {
    unmount(app);
    target.remove();
  });
  await listCommands()
    .find((c) => c.id === "settings.claude")
    ?.run();
  await settle();
  const field = () => target.querySelector<HTMLInputElement>("[data-testid=agent-limit] input");
  expect(field()?.value).toBe("3");
  expect(target.querySelector("[data-testid=agent-limit]")?.textContent).toContain("Claude usage limits");
  const change = async (value: string) => {
    const input = field();
    if (!input) throw new Error("no field");
    input.value = value;
    input.dispatchEvent(new Event("change", { bubbles: true }));
    await settle();
  };
  await change("5");
  expect(field()?.value).toBe("5");
  await change("12");
  expect(sent).toEqual([5, 12]);
  expect(field()?.value).toBe("5");
  expect(target.querySelector("[data-testid=agent-limit] [role=alert]")?.textContent).toContain("between 1 and 8");
  // An empty field is not sent; it shows the kept number again.
  await change("");
  expect(sent).toEqual([5, 12]);
  expect(field()?.value).toBe("5");
  expect(target.querySelector("[data-testid=agent-limit] [role=alert]")?.textContent).toContain("from 1 to 8");
});

it("turns the agents' resource usage on and off, off at first", async () => {
  const target = document.createElement("div");
  document.body.append(target);
  const app = mount(TestHost, { target, props: { transport, workspace: new Workspace() } });
  cleanup.push(registerSettingsCommands(), () => {
    unmount(app);
    target.remove();
    agentPrefs.showResources = false;
  });
  await listCommands()
    .find((c) => c.id === "settings.claude")
    ?.run();
  await settle();
  const toggle = target.querySelector<HTMLInputElement>("[data-testid=show-resources]");
  expect(toggle?.checked).toBe(false);
  toggle?.click();
  await settle();
  expect(agentPrefs.showResources).toBe(true);
});
