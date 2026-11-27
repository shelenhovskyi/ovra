import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { basename } from "node:path";
import { test as base, expect, type Locator, type Page } from "@playwright/test";
import type { AxeResults, RunOptions } from "axe-core";

// The desktop window runs with Tauri's `freezePrototype` (app/tauri.conf.json),
// so every page here does too: code that assigns a property Object.prototype
// already has (`obj.toString = f`) throws there and blanks the window, while a
// plain browser lets it pass. Uncaught page errors fail the test for the same
// reason: in the window nothing else would show them.
export const test = base.extend({
  context: async ({ context }, use) => {
    await context.addInitScript(() => Object.freeze(Object.prototype));
    await use(context);
  },
  page: async ({ page }, use) => {
    const errors: string[] = [];
    page.on("pageerror", (error) => errors.push(error.message));
    await use(page);
    expect(errors, "uncaught errors in the page").toEqual([]);
  },
});

export { expect };

/**
 * Clicks a button that counts a press only a moment after what it confirms
 * last changed (approvals, project trust), by moving the page's clock on
 * instead of waiting for it. The page must have run `page.clock.install()`
 * before it loaded. Moves the clock again while something new keeps arriving.
 */
export async function clickWhenArmed(page: Page, button: Locator): Promise<void> {
  await expect(async () => {
    await page.clock.fastForward(600);
    await expect(button).toHaveAttribute("aria-disabled", "false", { timeout: 100 });
  }).toPass({ intervals: [0] });
  await button.click();
}

/** A shortcut as the UI labels it on this OS: `⌥P` on a Mac, `Alt+P` elsewhere. */
export function shortcutLabel(text: string): string {
  if (process.platform !== "darwin") return text;
  const parts = text.split("+");
  const key = parts.pop() ?? "";
  const symbols: Record<string, string> = { Ctrl: "⌃", Alt: "⌥", Shift: "⇧", Meta: "⌘" };
  return (
    ["Ctrl", "Alt", "Shift", "Meta"]
      .filter((m) => parts.includes(m))
      .map((m) => symbols[m])
      .join("") + key
  );
}

/** The shared daemon's URL, from globalSetup. */
function daemonUrl(): string {
  const url = process.env.OVRA_URL;
  if (!url) throw new Error("OVRA_URL is not set; globalSetup did not run");
  return url;
}

/** Calls the daemon over its WebSocket, one call after another; resolves with each call's data. */
async function callCore(url: string, calls: { method: string; params: unknown }[]): Promise<unknown[]> {
  const { host, searchParams } = new URL(url);
  const socket = new WebSocket(`ws://${host}/ws?token=${encodeURIComponent(searchParams.get("token") ?? "")}`);
  try {
    await new Promise((ok, fail) => {
      socket.addEventListener("open", ok, { once: true });
      socket.addEventListener("error", fail, { once: true });
    });
    const answers: unknown[] = [];
    for (const [n, { method, params }] of calls.entries()) {
      answers.push(
        await new Promise((ok, fail) => {
          const listen = (event: MessageEvent) => {
            const frame = JSON.parse(String(event.data)) as {
              id?: number;
              result?: { data: unknown };
              error?: unknown;
            };
            if (frame.id !== n + 1) return;
            socket.removeEventListener("message", listen);
            if (frame.result) ok(frame.result.data);
            else fail(new Error(`${method} failed: ${JSON.stringify(frame.error)}`));
          };
          socket.addEventListener("message", listen);
          socket.send(JSON.stringify({ id: n + 1, method, params }));
        }),
      );
    }
    return answers;
  } finally {
    socket.close();
  }
}

/** Adds `root` to the daemon's projects over its WebSocket; resolves with the project's id. */
export async function addProject(url: string, root: string): Promise<string> {
  const [project] = await callCore(url, [{ method: "projects.add", params: { root } }]);
  return (project as { id: string }).id;
}

/**
 * Stops every agent session of the project `page` has open. The daemon is
 * shared, and the approvals of every project reach every page: an agent left
 * running could ask again during a later test, whose card would then not be
 * the one on screen.
 */
export async function stopAgents(page: Page, url = daemonUrl()): Promise<void> {
  const project = await page.evaluate(() => localStorage.getItem("ovra.project")).catch(() => null);
  if (!project) return;
  const [sessions] = (await callCore(url, [{ method: "agents.list", params: { project } }])) as {
    id: string;
    status: { state: string };
  }[][];
  const running = (sessions ?? []).filter((s) => !["stopped", "failed"].includes(s.status.state));
  await callCore(
    url,
    running.map((s) => ({ method: "agents.stop", params: { session: s.id } })),
  );
}

/**
 * Loads the page with `dir` open as its project, the way a reload opens the
 * project of the last visit: about 0.1 s faster than the start page's form,
 * which tests of that form (tree.spec.ts, shell.spec.ts) still use.
 */
export async function loadProject(page: Page, dir: string, url = daemonUrl()): Promise<void> {
  const id = await addProject(url, dir);
  // Only on the first load: a later reload opens what the test left open.
  await page.addInitScript((project) => {
    if (sessionStorage.getItem("e2e.project")) return;
    sessionStorage.setItem("e2e.project", project);
    localStorage.setItem("ovra.project", project);
  }, id);
  await page.goto(url);
  await expect(page.getByRole("button", { name: `Project ${basename(dir)}, switch project` })).toBeVisible();
}

/** axe-core's script, put into a page the first time it is checked. */
const axeSource = readFileSync(createRequire(import.meta.url).resolve("axe-core/axe.min.js"), "utf8");

/**
 * Fails when axe finds accessibility problems on the page as it is now (WCAG
 * 2.2 A and AA plus axe's best practices), in the colour scheme and contrast
 * the page emulates. Each problem is listed with the elements it was found on.
 */
export async function expectAccessible(page: Page, where: string): Promise<void> {
  if (!(await page.evaluate(() => "axe" in window))) await page.evaluate(axeSource);
  const options: RunOptions = {
    runOnly: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa", "best-practice"],
    // The panel splitters sit between the landmarks, as grid cells of their
    // own; each is a labelled separator, so a screen reader still finds it.
    rules: { region: { enabled: false } },
    resultTypes: ["violations"],
  };
  const violations = await page.evaluate(async (given) => {
    const { axe } = window as unknown as {
      axe: { run: (context: Document, options: RunOptions) => Promise<AxeResults> };
    };
    // axe settles each rule on a zero-delay timer. Under the page clock of
    // `page.clock.install()` those fire one after another, about 0.4 s per
    // run on an agent screen; a message is a task of its own that the clock
    // does not hold. Pages without the clock keep the browser's timers.
    const timer = window.setTimeout;
    const clocked = !/\[native code\]/.test(Function.prototype.toString.call(timer));
    const channel = new MessageChannel();
    const due: (() => void)[] = [];
    channel.port1.onmessage = () => due.shift()?.();
    if (clocked)
      window.setTimeout = ((handler: TimerHandler, delay?: number, ...args: unknown[]) => {
        if (delay || typeof handler !== "function") return timer(handler, delay, ...args);
        due.push(() => handler(...args));
        channel.port2.postMessage(null);
        return 0;
      }) as typeof window.setTimeout;
    const { violations } = await axe.run(document, given).finally(() => {
      window.setTimeout = timer;
      channel.port1.close();
    });
    return violations.map(({ id, impact, help, nodes }) => ({ id, impact, help, nodes: nodes.map((n) => n.target) }));
  }, options);
  const found = violations.map(
    (v) => `${v.id} (${v.impact}): ${v.help}\n${v.nodes.map((target) => `    ${target.join(" ")}`).join("\n")}`,
  );
  expect(found, `accessibility problems ${where}`).toEqual([]);
}

/** Presses Tab (Shift+Tab with `back`) until `target` has focus; fails after `limit` presses. */
export async function tabTo(page: Page, target: Locator, { back = false, limit = 25 } = {}): Promise<void> {
  for (let presses = 0; presses < limit; presses++) {
    if (await target.evaluate((element) => element === document.activeElement)) return;
    await page.keyboard.press(back ? "Shift+Tab" : "Tab");
  }
  await expect(target, `not reached with ${limit} presses`).toBeFocused({ timeout: 0 });
}
