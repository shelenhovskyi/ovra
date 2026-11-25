import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { installKeybindings } from "../keys/keybindings";
import type { FrameMetadata, PageInfo } from "../lib/protocol";
import { Workspace } from "../shell/workspace.svelte";
import { FakeBrowserCore } from "./fake-browser";
import TestHost from "./TestHost.svelte";

let app: Record<string, unknown> | null = null;
let target: HTMLElement;
let uninstall: () => void = () => {};
/** Whatever IntersectionObserver the view made last. */
let seen: ((entries: { isIntersecting: boolean }[]) => void) | null = null;
const ctx = { clearRect: vi.fn(), drawImage: vi.fn() };
const decode = vi.fn(async (_blob: Blob) => ({ width: 1280, height: 720, close() {} }));

beforeEach(() => {
  vi.useFakeTimers();
  target = document.createElement("div");
  document.body.append(target);
  // Every element is a 640 by 600 box at the corner of the window.
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue(
    DOMRect.fromRect({ x: 0, y: 0, width: 640, height: 600 }),
  );
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(ctx as unknown as CanvasRenderingContext2D);
  vi.stubGlobal("createImageBitmap", decode);
  vi.stubGlobal(
    "IntersectionObserver",
    class {
      constructor(callback: (entries: { isIntersecting: boolean }[]) => void) {
        seen = callback;
      }
      observe() {}
      disconnect() {}
    },
  );
  ctx.drawImage.mockClear();
  decode.mockClear();
});

afterEach(() => {
  if (app) unmount(app);
  app = null;
  uninstall();
  target.remove();
  seen = null;
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

async function settle() {
  for (let i = 0; i < 4; i++) {
    flushSync();
    await vi.advanceTimersByTimeAsync(0);
  }
  flushSync();
}

const PAGE: PageInfo = { id: "pg1", session: "s1", url: "http://localhost:5173/", title: "Home" };
const META: FrameMetadata = { width: 1280, height: 720, scroll_x: 0, scroll_y: 0 };

/** Shows the pane with one page that has had a frame, on screen. */
async function setup(core = new FakeBrowserCore(), { frame = true } = {}) {
  const workspace = new Workspace();
  workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: false };
  app = mount(TestHost, { target, props: { transport: core, workspace } });
  uninstall = installKeybindings(window, false, false);
  await settle();
  key("keydown", { code: "KeyB", key: "b", ctrlKey: true, shiftKey: true });
  await settle();
  core.pages("s1", [PAGE]);
  await settle();
  if (frame) {
    seen?.([{ isIntersecting: true }]);
    core.emit({ type: "browser.frame", data: { page: "pg1", jpeg: btoa("jpeg"), metadata: META } });
    await settle();
  }
  return core;
}

const $ = <T extends HTMLElement>(selector: string) => target.querySelector<T>(selector);
const button = (name: string) =>
  [...target.querySelectorAll<HTMLButtonElement>("button")].find(
    (b) => b.getAttribute("aria-label") === name || b.textContent?.trim() === name,
  );
const key = (type: string, init: KeyboardEventInit) => {
  const event = new KeyboardEvent(type, { bubbles: true, cancelable: true, ...init });
  (document.activeElement ?? document.body).dispatchEvent(event);
  return event;
};

describe("the agent browser", () => {
  it("opens with its shortcut and shows the session's pages as tabs", async () => {
    const core = await setup(new FakeBrowserCore(), { frame: false });
    expect(document.activeElement?.id).toBe("browser-title");
    const tabs = () => [...target.querySelectorAll<HTMLButtonElement>("[role=tab]")];
    core.pages("s1", [PAGE, { ...PAGE, id: "pg2", title: "", url: "http://localhost:5173/login" }]);
    await settle();
    expect(tabs().map((t) => [t.textContent, t.getAttribute("aria-selected")])).toEqual([
      ["Home", "true"],
      ["http://localhost:5173/login", "false"],
    ]);
    expect($<HTMLInputElement>("#browser-address")?.value).toBe("http://localhost:5173/");
    tabs()[0]?.focus();
    key("keydown", { code: "ArrowRight", key: "ArrowRight" });
    await settle();
    expect(document.activeElement).toBe(tabs()[1]);
    expect($<HTMLInputElement>("#browser-address")?.value).toBe("http://localhost:5173/login");
    button("Close http://localhost:5173/login")?.click();
    await settle();
    expect(core.sent("browser.close")).toEqual([{ page: "pg2" }]);
    expect(tabs().map((t) => t.textContent)).toEqual(["Home"]);
  });

  it("draws the newest frame scaled to fit and hides with its shortcut", async () => {
    const core = await setup(new FakeBrowserCore(), { frame: false });
    expect($("[data-testid=browser-canvas]")?.getAttribute("aria-label")).toBe("Home, http://localhost:5173/");
    seen?.([{ isIntersecting: true }]);
    for (const jpeg of ["one", "two", "three"])
      core.emit({ type: "browser.frame", data: { page: "pg1", jpeg: btoa(jpeg), metadata: META } });
    core.emit({ type: "browser.frame", data: { page: "other", jpeg: btoa("x"), metadata: META } });
    await settle();
    // The first decodes at once; of the two that came meanwhile only the newest.
    expect(decode).toHaveBeenCalledTimes(2);
    // 1280 by 720 in a 640 by 600 box: half size, centred.
    expect(ctx.drawImage).toHaveBeenLastCalledWith(expect.anything(), 0, 120, 640, 360);
    key("keydown", { code: "KeyB", key: "b", ctrlKey: true, shiftKey: true });
    await settle();
    expect($("[data-testid=browser]")).toBeNull();
    expect(document.activeElement).toBe($("[data-testid=open-browser]"));
  });

  it("asks for frames only while the page can be seen", async () => {
    const core = await setup(new FakeBrowserCore(), { frame: false });
    const casts = () => core.sent("browser.screencast").map((p) => p.on);
    expect(casts()).toEqual([]);
    seen?.([{ isIntersecting: true }]);
    await settle();
    expect(casts()).toEqual([true]);
    vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");
    document.dispatchEvent(new Event("visibilitychange"));
    await settle();
    expect(casts()).toEqual([true, false]);
    vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
    document.dispatchEvent(new Event("visibilitychange"));
    await settle();
    button("Hide")?.click();
    await settle();
    expect(casts()).toEqual([true, false, true, false]);
  });

  it("asks the person before an agent opens another origin", async () => {
    const core = await setup();
    button("Hide")?.click();
    await settle();
    core.emit({
      type: "browser.navigation_asked",
      data: { page: "pg1", session: "s1", origin: "https://example.com" },
    });
    await settle();
    expect(button("Browser, 1 waiting for you")).toBeDefined();
    button("Browser, 1 waiting for you")?.click();
    await settle();
    const ask = $('[role=group][aria-label="Let the agent open https://example.com?"]');
    expect(ask?.textContent).toContain("Let the agent open https://example.com?");
    button("Allow")?.click();
    await settle();
    expect(core.sent("browser.answer_navigation")).toEqual([
      { page: "pg1", origin: "https://example.com", allow: true },
    ]);
    expect($("[data-testid=browser-ask]")).toBeNull();
  });

  it("explains why the browser cannot run here", async () => {
    const core = new FakeBrowserCore();
    core.refuse.set("browser.open", { code: "unavailable", message: "no Chromium found" });
    await setup(core, { frame: false });
    const field = $<HTMLInputElement>("#browser-address");
    if (!field) throw new Error("no address field");
    field.value = "localhost:5173/login";
    field.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    button("Open in new tab")?.click();
    await settle();
    expect(core.sent("browser.open")).toEqual([{ session: "s1", url: "http://localhost:5173/login" }]);
    const note = $("[data-testid=browser-unavailable]")?.textContent ?? "";
    expect(note).toContain("never downloads one");
    expect(note).toContain("Linux sandbox");
    expect(note).toContain("no Chromium found");
  });
});
