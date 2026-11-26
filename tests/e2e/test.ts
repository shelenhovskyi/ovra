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

/** Adds `root` to the daemon's projects over its WebSocket; resolves with the project's id. */
export async function addProject(url: string, root: string): Promise<string> {
  const { host, searchParams } = new URL(url);
  const socket = new WebSocket(`ws://${host}/ws?token=${encodeURIComponent(searchParams.get("token") ?? "")}`);
  try {
    await new Promise((ok, fail) => {
      socket.addEventListener("open", ok, { once: true });
      socket.addEventListener("error", fail, { once: true });
    });
    return await new Promise<string>((ok, fail) => {
      socket.addEventListener("message", (event) => {
        const frame = JSON.parse(String(event.data)) as {
          id?: number;
          result?: { data: { id: string } };
          error?: unknown;
        };
        if (frame.id !== 1) return;
        if (frame.result) ok(frame.result.data.id);
        else fail(new Error(`projects.add failed: ${JSON.stringify(frame.error)}`));
      });
      socket.send(JSON.stringify({ id: 1, method: "projects.add", params: { root } }));
    });
  } finally {
    socket.close();
  }
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
    const { violations } = await axe.run(document, given);
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
