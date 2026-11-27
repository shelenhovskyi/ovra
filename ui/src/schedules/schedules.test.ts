import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, it } from "vitest";
import type { CoreEvent, Request, Schedule, ScheduleDraft } from "../lib/protocol";
import type { Transport } from "../lib/transport";
import { settings } from "../settings/panes.svelte";
import TestHost from "../settings/TestHost.svelte";
import { mainView } from "../shell/main-view.svelte";
import { Workspace } from "../shell/workspace.svelte";
import { lastText, momentText, whenText } from "./words";

const NOW = new Date(2026, 10, 27, 10, 0).getTime();

it("says when a schedule runs, when next and how the last run went", () => {
  const when = (days: number[], time = "03:00") => whenText({ days, time, time_zone: "UTC" });
  expect(when([1, 2, 3, 4, 5, 6, 7])).toBe("Every day at 03:00");
  expect(when([5, 4, 3, 2, 1], "09:30")).toBe("Weekdays at 09:30");
  expect(when([6, 7])).toBe("Weekends at 03:00");
  expect(when([3, 1])).toBe("Mon, Wed at 03:00");
  expect(momentText(new Date(2026, 10, 27, 23, 5).getTime(), NOW)).toBe("today 23:05");
  expect(momentText(new Date(2026, 10, 28, 3, 0).getTime(), NOW)).toBe("tomorrow 03:00");
  expect(momentText(new Date(2026, 10, 30, 3, 0).getTime(), NOW)).toBe("Mon 30 Nov, 03:00");
  const at_ms = new Date(2026, 10, 27, 3, 0).getTime();
  expect(lastText({ at_ms, outcome: "started", session: "s1" }, NOW)).toBe("Last run started today 03:00");
  expect(lastText({ at_ms, outcome: "skipped", reason: "Busy." }, NOW)).toBe("Skipped today 03:00: Busy.");
  expect(lastText({ at_ms, outcome: "missed" }, NOW)).toContain("Missed today 03:00");
});

/** A core with schedules: off until switched on, keeps what is saved. */
function fakeCore() {
  const state = { on: false, list: [] as Schedule[], asked: [] as Request[] };
  const listeners = new Set<(e: CoreEvent) => void>();
  const schedule = (id: string, draft: ScheduleDraft): Schedule => ({
    ...draft,
    id,
    project: "p1",
    paused: false,
    next_ms: state.on ? NOW + 3_600_000 : null,
    last: null,
    last_session: null,
  });
  const call = (async (request: Request) => {
    state.asked.push(request);
    switch (request.method) {
      case "schedules.enabled":
        return { type: "schedules.enabled", data: { enabled: state.on } };
      case "schedules.set_enabled":
        state.on = request.params.enabled;
        return { type: "schedules.enabled", data: { enabled: state.on } };
      case "schedules.list":
        return { type: "schedules.list", data: state.list };
      case "schedules.save": {
        const saved = schedule(`sc${state.list.length + 1}`, request.params.draft);
        state.list = [...state.list, saved];
        return { type: "schedules.schedule", data: saved };
      }
      case "schedules.run_now": {
        const ran = state.list[0] as Schedule;
        const last = { at_ms: Date.now(), outcome: "started" as const, session: "s1" };
        return { type: "schedules.schedule", data: { ...ran, last } };
      }
      case "agents.providers":
        return { type: "agents.providers", data: [] };
      case "info.get":
        return { type: "info", data: { name: "ovra", version: "1", mode: "headless" } };
      default:
        throw new Error(`unexpected ${request.method}`);
    }
  }) as Transport["call"];
  const transport: Transport = {
    call,
    onEvent: (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
  return { state, transport };
}

let cleanup: (() => void)[] = [];

afterEach(() => {
  mainView.close(false);
  for (const off of cleanup) off();
  cleanup = [];
});

async function settle(): Promise<void> {
  for (let i = 0; i < 3; i++) {
    flushSync();
    await new Promise((resolve) => setImmediate(resolve));
  }
  flushSync();
}

it("shows the Schedules pane once switched on, and adds a schedule in this computer's time zone", async () => {
  const { state, transport } = fakeCore();
  const target = document.createElement("div");
  document.body.append(target);
  const workspace = new Workspace();
  workspace.project = { id: "p1", name: "p1", root: "/p1", last_opened_ms: null, trusted: false };
  const app = mount(TestHost, { target, props: { transport, workspace } });
  cleanup.push(() => {
    unmount(app);
    target.remove();
  });
  settings.pane = "claude";
  mainView.show("settings");
  await settle();
  const paneNames = () => [...target.querySelectorAll("nav button")].map((b) => b.textContent);
  expect(paneNames()).not.toContain("Schedules");
  const toggle = target.querySelector<HTMLInputElement>("[data-testid=schedules-switch]");
  expect(toggle?.checked).toBe(false);
  if (!toggle) throw new Error("no switch");
  toggle.checked = true;
  toggle.dispatchEvent(new Event("change", { bubbles: true }));
  await settle();
  expect(state.on).toBe(true);
  expect(paneNames()).toContain("Schedules");

  target.querySelector<HTMLButtonElement>("nav [data-pane=schedules]")?.click();
  await settle();
  const pane = () => target.querySelector("[data-testid=schedules-pane]");
  expect(pane()?.querySelector("[data-testid=schedules-empty]")).not.toBeNull();
  pane()?.querySelector<HTMLButtonElement>("[data-testid=schedules-empty] button")?.click();
  await settle();
  const form = pane()?.querySelector<HTMLFormElement>("[data-testid=schedule-form]");
  if (!form) throw new Error("no form");
  const fill = (selector: string, value: string) => {
    const input = form.querySelector<HTMLInputElement | HTMLTextAreaElement>(selector);
    if (!input) throw new Error(`no ${selector}`);
    input.value = value;
    input.dispatchEvent(new Event("input", { bubbles: true }));
  };
  fill("[data-field=name]", "Nightly bugs");
  fill("textarea", "Look for bugs");
  fill("[data-field=time]", "03:00");
  // Saturday and Sunday off.
  const days = [...form.querySelectorAll<HTMLInputElement>(".day input")];
  for (const day of days.slice(5)) day.click();
  form.requestSubmit();
  await settle();
  const saved = state.asked.find((r) => r.method === "schedules.save");
  expect(saved?.method === "schedules.save" && saved.params.draft).toEqual({
    name: "Nightly bugs",
    prompt: "Look for bugs",
    kind: "research",
    agent: "claude",
    when: { days: [1, 2, 3, 4, 5], time: "03:00", time_zone: Intl.DateTimeFormat().resolvedOptions().timeZone },
  });
  const row = pane()?.querySelector("[data-testid=schedule-row]");
  expect(row?.textContent).toContain("Weekdays at 03:00");
  expect(row?.querySelector("[data-testid=schedule-next]")?.textContent).toContain("Next run");

  [...(row?.querySelectorAll("button") ?? [])].find((b) => b.textContent === "Run now")?.click();
  await settle();
  expect(pane()?.querySelector("[data-testid=schedule-last]")?.textContent).toContain("Last run started");
});
