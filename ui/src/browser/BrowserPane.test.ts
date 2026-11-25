import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { installKeybindings } from "../keys/keybindings";
import type { FrameMetadata, PageInfo } from "../lib/protocol";
import { Workspace } from "../shell/workspace.svelte";
import { FakeBrowserCore } from "./fake-browser";
import { browserPicks } from "./picks.svelte";
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
const stage = () => $("[data-testid=browser-stage]") as HTMLElement;
const button = (name: string) =>
  [...target.querySelectorAll<HTMLButtonElement>("button")].find(
    (b) => b.getAttribute("aria-label") === name || b.textContent?.trim() === name,
  );
const mouse = (type: string, x: number, y: number, init: MouseEventInit = {}) =>
  stage().dispatchEvent(new MouseEvent(type, { clientX: x, clientY: y, bubbles: true, cancelable: true, ...init }));
const key = (type: string, init: KeyboardEventInit) => {
  const event = new KeyboardEvent(type, { bubbles: true, cancelable: true, ...init });
  (document.activeElement ?? document.body).dispatchEvent(event);
  return event;
};
const inputs = (core: FakeBrowserCore) => core.sent("browser.input").map((p) => p.event);
const box = (testid: string) => {
  const el = $(`[data-testid=${testid}]`);
  return el && [el.style.left, el.style.top, el.style.width, el.style.height];
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

  it("sends the person's mouse and keys in page pixels only while they control the page", async () => {
    const core = await setup();
    mouse("mousemove", 320, 300);
    expect(inputs(core)).toEqual([]);
    button("Take over")?.click();
    await settle();
    expect(document.activeElement).toBe(stage());
    expect($("[data-testid=browser-mode]")?.textContent).toContain("You are controlling the page");
    mouse("mousemove", 320, 300);
    mouse("mousedown", 160, 210, { button: 0, detail: 1 });
    mouse("mouseup", 160, 210, { button: 0, detail: 1 });
    stage().dispatchEvent(
      new WheelEvent("wheel", { clientX: 320, clientY: 300, deltaY: 3, deltaMode: 1, cancelable: true }),
    );
    key("keydown", { code: "KeyA", key: "a" });
    // Ovra's own shortcuts go to the page too.
    const save = key("keydown", { code: "KeyB", key: "b", ctrlKey: true, shiftKey: true });
    key("keyup", { code: "KeyA", key: "a" });
    await settle();
    expect(save.defaultPrevented).toBe(true);
    expect($("[data-testid=browser]")).not.toBeNull();
    expect(inputs(core)).toEqual([
      { kind: "mouse_move", x: 640, y: 360, modifiers: 0 },
      { kind: "mouse_down", x: 320, y: 180, button: "left", clicks: 1, modifiers: 0 },
      { kind: "mouse_up", x: 320, y: 180, button: "left", clicks: 1, modifiers: 0 },
      { kind: "wheel", x: 640, y: 360, delta_x: 0, delta_y: 48, modifiers: 0 },
      { kind: "key_down", key: "a", code: "KeyA", text: "a", modifiers: 0 },
      { kind: "key_down", key: "b", code: "KeyB", text: null, modifiers: 10 },
      { kind: "key_up", key: "a", code: "KeyA", modifiers: 0 },
    ]);
    // Focus elsewhere pauses control and lets go of the keys still down; Shift+Escape ends it.
    stage().blur();
    await settle();
    mouse("mousemove", 10, 200);
    expect(inputs(core).slice(7)).toEqual([{ kind: "key_up", key: "b", code: "KeyB", modifiers: 0 }]);
    expect($("[data-testid=browser-mode]")?.textContent).toBe("Click the page to control it again.");
    stage().focus();
    key("keydown", { code: "Escape", key: "Escape", shiftKey: true });
    await settle();
    expect(inputs(core)).toHaveLength(8);
    expect(button("Take over")?.getAttribute("aria-pressed")).toBe("false");
  });

  it("releases the keys it pressed in the page when control ends", async () => {
    const core = await setup();
    button("Take over")?.click();
    await settle();
    key("keydown", { code: "ShiftLeft", key: "Shift", shiftKey: true });
    key("keydown", { code: "Escape", key: "Escape", shiftKey: true });
    await settle();
    expect(inputs(core).slice(-1)).toEqual([{ kind: "key_up", key: "Shift", code: "ShiftLeft", modifiers: 0 }]);
    key("keyup", { code: "ShiftLeft", key: "Shift" });
    await settle();
    expect(inputs(core)).toHaveLength(2);
    // Focus leaving the page mid-chord releases the keys too; Alt chords type nothing.
    button("Give back")?.click();
    button("Take over")?.click();
    await settle();
    key("keydown", { code: "ControlLeft", key: "Control", ctrlKey: true });
    key("keydown", { code: "KeyP", key: "P", altKey: true, shiftKey: true });
    stage().blur();
    await settle();
    expect(inputs(core).slice(2)).toEqual([
      { kind: "key_down", key: "Control", code: "ControlLeft", text: null, modifiers: 2 },
      { kind: "key_down", key: "P", code: "KeyP", text: null, modifiers: 9 },
      { kind: "key_up", key: "P", code: "KeyP", modifiers: 2 },
      { kind: "key_up", key: "Control", code: "ControlLeft", modifiers: 0 },
    ]);
  });

  it("survives its page closing with the session and shows the next one", async () => {
    const core = await setup();
    const errors: unknown[] = [];
    const onError = (event: ErrorEvent) => errors.push(event.error);
    window.addEventListener("error", onError);
    core.pages("s1", []);
    await settle();
    window.removeEventListener("error", onError);
    expect(errors).toEqual([]);
    expect(core.sent("browser.screencast").at(-1)).toEqual({ page: "pg1", on: false });
    expect(target.textContent).toContain("No pages open");
    core.pages("s1", [{ ...PAGE, id: "pg2" }]);
    await settle();
    expect(stage()).not.toBeNull();
  });

  it("picks elements with an overlay of its own, Arrow Up and Down, and notes", async () => {
    const core = await setup();
    key("keydown", { code: "KeyP", key: "P", altKey: true, shiftKey: true });
    await settle();
    expect(button("Pick")?.getAttribute("aria-pressed")).toBe("true");
    mouse("mousemove", 320, 300);
    mouse("mousemove", 330, 300);
    await settle();
    // One hit test until the throttle lets the next through.
    expect(core.sent("browser.pick")).toEqual([{ page: "pg1", x: 640, y: 360, up: 0 }]);
    // The button's 100 by 40 box at the page's corner, at half size.
    expect(box("browser-hover-box")).toEqual(["0px", "120px", "50px", "20px"]);
    await vi.advanceTimersByTimeAsync(100);
    await settle();
    expect(core.sent("browser.pick").at(-1)).toEqual({ page: "pg1", x: 660, y: 360, up: 0 });
    key("keydown", { code: "ArrowUp", key: "ArrowUp" });
    await settle();
    expect(core.sent("browser.pick").at(-1)?.up).toBe(1);
    expect(box("browser-hover-box")).toEqual(["5px", "125px", "50px", "20px"]);
    expect($("[data-testid=browser-hover-box]")?.textContent).toContain('div1 "div1"');
    key("keydown", { code: "ArrowDown", key: "ArrowDown" });
    await settle();
    expect(core.sent("browser.pick").at(-1)?.up).toBe(0);
    // Shift+click adds and keeps picking; a plain click adds and stops.
    mouse("click", 330, 300, { shiftKey: true });
    await settle();
    core.element = (_x, _y, up) => pickOf("a", up);
    mouse("click", 100, 200);
    await settle();
    expect(button("Pick")?.getAttribute("aria-pressed")).toBe("false");
    expect($("[data-testid=browser-hover-box]")).toBeNull();
    expect(target.querySelectorAll("[data-testid=browser-picked-box]")).toHaveLength(2);
    expect(inputs(core)).toEqual([]);
    const note = $<HTMLInputElement>('input[aria-label="Note for button \\"button\\""]');
    if (!note) throw new Error("no note field");
    note.value = "make it blue";
    note.dispatchEvent(new Event("input", { bubbles: true }));
    const picked = browserPicks(core).of("s1");
    expect(picked.map((p) => [p.page, p.pick.tag, p.pick.note])).toEqual([
      ["pg1", "button", "make it blue"],
      ["pg1", "a", ""],
    ]);
    // Escape leaves pick mode.
    button("Pick")?.click();
    await settle();
    key("keydown", { code: "Escape", key: "Escape" });
    await settle();
    expect(button("Pick")?.getAttribute("aria-pressed")).toBe("false");
  });

  it("picks without a mouse: arrows start at the page's centre, Enter picks", async () => {
    const core = await setup();
    key("keydown", { code: "KeyP", key: "P", altKey: true, shiftKey: true });
    await settle();
    expect(document.activeElement).toBe(stage());
    expect($("[data-testid=browser-mode]")?.textContent).toContain("Enter picks");
    // The first arrow shows the element at the centre, the next one its parent.
    key("keydown", { code: "ArrowUp", key: "ArrowUp" });
    await settle();
    expect(core.sent("browser.pick")).toEqual([{ page: "pg1", x: 640, y: 360, up: 0 }]);
    expect($("[data-testid=browser-hover-box]")?.textContent).toContain('button "button"');
    key("keydown", { code: "ArrowUp", key: "ArrowUp" });
    await settle();
    expect(core.sent("browser.pick").at(-1)?.up).toBe(1);
    // Shift+Enter picks and keeps picking; Enter picks and stops.
    key("keydown", { code: "Enter", key: "Enter", shiftKey: true });
    await settle();
    expect(button("Pick")?.getAttribute("aria-pressed")).toBe("true");
    key("keydown", { code: "ArrowDown", key: "ArrowDown" });
    await settle();
    key("keydown", { code: "Enter", key: "Enter" });
    await settle();
    expect(button("Pick")?.getAttribute("aria-pressed")).toBe("false");
    // Enter before any arrow picks the element at the centre.
    core.element = (_x, _y, up) => pickOf("a", up);
    key("keydown", { code: "KeyP", key: "P", altKey: true, shiftKey: true });
    await settle();
    key("keydown", { code: "NumpadEnter", key: "Enter" });
    await settle();
    expect(
      browserPicks(core)
        .of("s1")
        .map((p) => p.pick.tag),
    ).toEqual(["div1", "button", "a"]);
    expect(core.sent("browser.pick").at(-1)).toEqual({ page: "pg1", x: 640, y: 360, up: 0 });
    expect(inputs(core)).toEqual([]);
  });

  it("keeps focus on the pick list as picks are removed, then on Pick", async () => {
    const core = await setup();
    button("Pick")?.click();
    await settle();
    mouse("click", 320, 300, { shiftKey: true });
    await settle();
    core.element = (_x, _y, up) => pickOf("a", up);
    mouse("click", 100, 200);
    await settle();
    const remove = (label: string) => {
      const b = button(`Remove ${label}`);
      b?.focus();
      b?.click();
    };
    remove('button "button"');
    await settle();
    expect(document.activeElement).toBe(button('Remove link "a"'));
    remove('link "a"');
    await settle();
    expect(browserPicks(core).of("s1")).toEqual([]);
    expect(document.activeElement).toBe(button("Pick"));
  });

  it("stops hit tests when the pane hides in pick mode", async () => {
    const core = await setup();
    button("Pick")?.click();
    mouse("mousemove", 320, 300);
    mouse("mousemove", 330, 300);
    button("Hide")?.click();
    await settle();
    await vi.advanceTimersByTimeAsync(200);
    expect(core.sent("browser.pick")).toHaveLength(1);
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
    // Showing the pane with an ask waiting puts the person on it.
    expect(document.activeElement).toBe(button("Allow"));
    button("Allow")?.click();
    await settle();
    expect(core.sent("browser.answer_navigation")).toEqual([
      { page: "pg1", origin: "https://example.com", allow: true },
    ]);
    expect($("[data-testid=browser-ask]")).toBeNull();
    expect(document.activeElement?.textContent).toBe("Home");
  });

  it("moves focus to a new ask, and to the next one after an answer", async () => {
    const core = await setup();
    const asked = (origin: string) =>
      core.emit({ type: "browser.navigation_asked", data: { page: "pg1", session: "s1", origin } });
    const allow = (origin: string) =>
      $<HTMLButtonElement>(`[role=group][aria-label="Let the agent open ${origin}?"] button`);
    // Not while the person works the page itself.
    stage().focus();
    asked("https://a.test");
    await settle();
    expect(document.activeElement).toBe(stage());
    $<HTMLInputElement>("#browser-address")?.focus();
    asked("https://b.test");
    await settle();
    expect(document.activeElement).toBe(allow("https://b.test"));
    const deny = $<HTMLButtonElement>(
      '[role=group][aria-label="Let the agent open https://a.test?"] button:last-child',
    );
    deny?.focus();
    deny?.click();
    await settle();
    expect(core.sent("browser.answer_navigation")).toEqual([{ page: "pg1", origin: "https://a.test", allow: false }]);
    expect(document.activeElement).toBe(allow("https://b.test"));
  });

  it("says why the browser cannot run for one session, and only there", async () => {
    const core = new FakeBrowserCore();
    const [first] = core.sessions;
    if (!first) throw new Error("no session");
    core.sessions = [first, { ...first, id: "s2", title: "Other" }];
    const reason = "the agent browser runs only for sessions whose agent runs in the sandbox";
    core.refuse.set("browser.open", { code: "unavailable", message: reason });
    await setup(core, { frame: false });
    const field = $<HTMLInputElement>("#browser-address");
    if (!field) throw new Error("no address field");
    field.value = "localhost:5173/login";
    field.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    button("Open in new tab")?.click();
    await settle();
    expect(core.sent("browser.open")).toEqual([{ session: "s1", url: "http://localhost:5173/login" }]);
    const note = () => $("[data-testid=browser-unavailable]")?.textContent ?? null;
    expect(note()).toContain(`The agent browser runs only for sessions whose agent runs in the sandbox.`);
    expect(note()).not.toContain("Chromium");
    const choose = async (id: string) => {
      const select = $<HTMLSelectElement>("[data-testid=browser-session]");
      if (!select) throw new Error("no session select");
      select.value = id;
      select.dispatchEvent(new Event("change", { bubbles: true }));
      await settle();
    };
    await choose("s2");
    expect(note()).toBeNull();
    expect($("#browser-address")).not.toBeNull();
    // Coming back tries again instead of keeping the old refusal.
    await choose("s1");
    expect(note()).toBeNull();
    expect($("#browser-address")).not.toBeNull();
  });
});

function pickOf(tag: string, up: number) {
  return {
    pick_id: `pk-${tag}${up}`,
    url: PAGE.url,
    selector: `main > ${tag}`,
    locator: `link "${tag}"`,
    tag,
    text: "",
    html: "",
    rect: { x: 200, y: 200, width: 20, height: 20 },
  };
}
