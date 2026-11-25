<script lang="ts">
// One page of a session's browser: its screencast on a canvas, scaled to fit
// and asked for only while the canvas can be seen; the person's mouse and keys
// while they control it; and pick mode with Ovra's overlay (PickOverlay).
import { untrack } from "svelte";
import { registerCommand } from "../keys/commands";
import { formatShortcut, isApple } from "../keys/shortcut";
import type { FrameMetadata, InputEvent, MouseButton, PageInfo } from "../lib/protocol";
import { useOvra } from "../shell/context";
import { sessionBrowsers } from "./browser.svelte";
import { fitFrame, modifiers, toPage } from "./geometry";
import PickList from "./PickList.svelte";
import PickOverlay from "./PickOverlay.svelte";
import { Picker } from "./picker.svelte";
import { browserPicks } from "./picks.svelte";

let { page }: { page: PageInfo } = $props();

const PICK_KEYS = "Alt+Shift+P";
const BUTTONS: (MouseButton | undefined)[] = ["left", "middle", "right"];

const { transport } = useOvra();
const browsers = sessionBrowsers(transport);
const picks = browserPicks(transport);
// The pane mounts a new view for every page, and its cleanups run after the
// pane's page is gone: they use the id and session it had.
// svelte-ignore state_referenced_locally
const { id, session } = page;
const picker = new Picker(transport, id);
$effect(() => () => picker.stop());

let stage = $state<HTMLElement>();
let canvas = $state<HTMLCanvasElement>();
let box = $state({ width: 0, height: 0 });
let meta = $state<FrameMetadata | null>(null);
let intersecting = $state(typeof IntersectionObserver === "undefined");
let tabVisible = $state(document.visibilityState !== "hidden");
let control = $state(false);
let focused = $state(false);

const frame = $derived(meta ? fitFrame(box, meta) : null);
const controlling = $derived(control && focused);
const alt = $derived(`${page.title || "Untitled page"}, ${page.url}`);
const pickKeys = formatShortcut(PICK_KEYS, isApple());
const modeText = $derived(
  picker.active
    ? "Picking: point at an element. Up and Down arrows choose its parent or child, click picks it, Shift+click picks more, Escape stops."
    : controlling
      ? "You are controlling the page. Shift+Escape gives it back."
      : control
        ? "Click the page to control it again."
        : "The agent drives this page. Take over to use your mouse and keyboard on it.",
);

// Frames: decode the newest one, skip any that arrived meanwhile.
let next: { jpeg: string; metadata: FrameMetadata } | null = null;
let decoding = false;
let bitmap: ImageBitmap | null = null;

$effect(() =>
  transport.onEvent((event) => {
    if (event.type !== "browser.frame" || event.data.page !== id) return;
    next = event.data;
    if (!decoding) void decode();
  }),
);

$effect(() => () => {
  bitmap?.close();
  bitmap = null;
});

async function decode(): Promise<void> {
  decoding = true;
  try {
    while (next) {
      const { jpeg, metadata } = next;
      next = null;
      try {
        const bytes = Uint8Array.from(atob(jpeg), (c) => c.charCodeAt(0));
        const decoded = await createImageBitmap(new Blob([bytes], { type: "image/jpeg" }));
        bitmap?.close();
        bitmap = decoded;
        meta = metadata;
        draw();
      } catch {
        // A frame that does not decode: keep showing the last one.
      }
    }
  } finally {
    decoding = false;
  }
}

function draw(): void {
  const ctx = canvas?.getContext("2d");
  if (!canvas || !ctx || !bitmap || !frame) return;
  const dpr = window.devicePixelRatio || 1;
  ctx.clearRect(0, 0, canvas.width, canvas.height);
  ctx.drawImage(bitmap, frame.x * dpr, frame.y * dpr, frame.width * dpr, frame.height * dpr);
}

function measure(): void {
  const rect = stage?.getBoundingClientRect();
  if (rect) box = { width: rect.width, height: rect.height };
}

$effect(() => {
  const el = stage;
  if (!el) return;
  measure();
  if (typeof ResizeObserver === "undefined") return;
  const observer = new ResizeObserver(measure);
  observer.observe(el);
  return () => observer.disconnect();
});

// The canvas has as many pixels as the screen shows; resizing clears it.
$effect(() => {
  if (!canvas) return;
  const dpr = window.devicePixelRatio || 1;
  canvas.width = Math.round(box.width * dpr);
  canvas.height = Math.round(box.height * dpr);
  untrack(draw);
});

// Frames flow only while the canvas is on screen and the window shows.
$effect(() => {
  const el = stage;
  if (!el || typeof IntersectionObserver === "undefined") return;
  const observer = new IntersectionObserver((entries) => {
    for (const entry of entries) intersecting = entry.isIntersecting;
  });
  observer.observe(el);
  return () => observer.disconnect();
});

$effect(() => {
  const update = () => (tabVisible = document.visibilityState !== "hidden");
  document.addEventListener("visibilitychange", update);
  return () => document.removeEventListener("visibilitychange", update);
});

let casting = false;

function screencast(on: boolean): void {
  if (casting === on) return;
  casting = on;
  const params = { page: id, on };
  browsers.guard(() => transport.call({ method: "browser.screencast", params })).catch(() => {});
}

$effect(() => {
  const on = intersecting && tabVisible;
  untrack(() => screencast(on));
});
$effect(() => () => untrack(() => screencast(false)));

$effect(() =>
  registerCommand({
    id: "browser.pick",
    title: "Pick an element in the agent's browser",
    keys: PICK_KEYS,
    run: togglePick,
  }),
);

function input(event: InputEvent): Promise<unknown> {
  return browsers.guard(() => transport.call({ method: "browser.input", params: { page: id, event } }));
}

function send(event: InputEvent): void {
  input(event).catch(() => {});
}

// Mouse moves wait for the one before: at most one is in flight.
let moving = false;
let nextMove: InputEvent | null = null;

function sendMove(event: InputEvent): void {
  if (moving) {
    nextMove = event;
    return;
  }
  moving = true;
  input(event)
    .catch(() => {})
    .finally(() => {
      moving = false;
      const queued = nextMove;
      nextMove = null;
      if (queued) sendMove(queued);
    });
}

function pointAt(event: MouseEvent): { x: number; y: number } | null {
  if (!stage || !frame) return null;
  const rect = stage.getBoundingClientRect();
  return toPage({ x: event.clientX - rect.left, y: event.clientY - rect.top }, frame);
}

function toggleControl(): void {
  control = !control;
  if (control) {
    picker.stop();
    stage?.focus();
  }
}

function togglePick(): void {
  if (picker.active) {
    picker.stop();
    return;
  }
  control = false;
  picker.start();
  stage?.focus();
}

function onMouseMove(event: MouseEvent): void {
  const at = pointAt(event);
  if (!at) return;
  if (picker.active) picker.hover(at);
  else if (controlling) sendMove({ kind: "mouse_move", ...at, modifiers: modifiers(event) });
}

function onMouseDown(event: MouseEvent): void {
  if (!picker.active && !control) return;
  // Keep the text selection and focus where Ovra wants them.
  event.preventDefault();
  stage?.focus();
  if (picker.active) return;
  const at = pointAt(event);
  const button = BUTTONS[event.button];
  if (at && button)
    send({ kind: "mouse_down", ...at, button, clicks: Math.max(1, event.detail), modifiers: modifiers(event) });
}

function onMouseUp(event: MouseEvent): void {
  if (!controlling) return;
  const at = pointAt(event);
  const button = BUTTONS[event.button];
  if (at && button)
    send({ kind: "mouse_up", ...at, button, clicks: Math.max(1, event.detail), modifiers: modifiers(event) });
}

async function onClick(event: MouseEvent): Promise<void> {
  if (!picker.active) return;
  const at = pointAt(event);
  if (!at) return;
  const more = event.shiftKey;
  const scroll = { x: meta?.scroll_x ?? 0, y: meta?.scroll_y ?? 0 };
  const pick = await picker.take(at);
  if (!pick) return;
  picks.add(session, id, pick, scroll);
  if (!more) picker.stop();
}

$effect(() => {
  const el = stage;
  if (!el) return;
  // Not passive: the page scrolls, not Ovra.
  const onWheel = (event: WheelEvent) => {
    if (!controlling) return;
    const at = pointAt(event);
    if (!at) return;
    event.preventDefault();
    const unit = event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? box.height : 1;
    send({
      kind: "wheel",
      ...at,
      delta_x: Math.round(event.deltaX * unit),
      delta_y: Math.round(event.deltaY * unit),
      modifiers: modifiers(event),
    });
  };
  el.addEventListener("wheel", onWheel, { passive: false });
  return () => el.removeEventListener("wheel", onWheel);
});

/** What a key types, as CDP wants it: Enter types a carriage return. Option types on Apple keyboards only. */
function typed(event: KeyboardEvent): string | null {
  if (event.ctrlKey || event.metaKey || (event.altKey && !isApple())) return null;
  if (event.key === "Enter") return "\r";
  return [...event.key].length === 1 ? event.key : null;
}

function onKeyDown(event: KeyboardEvent): void {
  if (picker.active) {
    if (event.code === "ArrowUp") picker.parent();
    else if (event.code === "ArrowDown") picker.child();
    else if (event.code === "Escape") picker.stop();
    else return;
    event.preventDefault();
    return;
  }
  if (!controlling || event.isComposing) return;
  // Every key goes to the page, Tab and Ovra's shortcuts too.
  event.preventDefault();
  if (event.code === "Escape" && event.shiftKey) {
    control = false;
    return;
  }
  send({ kind: "key_down", key: event.key, code: event.code, text: typed(event), modifiers: modifiers(event) });
  held.set(event.code, event.key);
}

function onKeyUp(event: KeyboardEvent): void {
  if (event.isComposing || !held.has(event.code)) return;
  event.preventDefault();
  held.delete(event.code);
  send({ kind: "key_up", key: event.key, code: event.code, modifiers: modifiers(event) });
}

/** Keys pressed in the page and not released there yet: `code` to `key`. */
const held = new Map<string, string>();
const MODIFIER_BITS: Record<string, number> = { Alt: 1, Control: 2, Meta: 4, Shift: 8 };

/** Releases every held key, newest first, so the page never keeps Shift down after control ends. */
function release(): void {
  for (const [code, key] of [...held].reverse()) {
    held.delete(code);
    const still = [...held.values()].reduce((bits, k) => bits | (MODIFIER_BITS[k] ?? 0), 0);
    send({ kind: "key_up", key, code, modifiers: still });
  }
}

$effect(() => {
  if (!controlling) untrack(release);
});
</script>

<div class="page-view">
  <div class="tools">
    <button type="button" aria-pressed={control} data-testid="browser-take-over" onclick={toggleControl}>
      {control ? "Give back" : "Take over"}
    </button>
    <button
      type="button"
      aria-pressed={picker.active}
      aria-keyshortcuts={PICK_KEYS}
      title={`Pick an element (${pickKeys})`}
      data-testid="browser-pick"
      onclick={togglePick}>Pick</button
    >
    <span class="mode" class:controlling role="status" data-testid="browser-mode">{modeText}</span>
  </div>
  <!-- The page itself: keys and the pointer go to it while the person controls it, so it is an
       `application` that takes focus, which Svelte counts as non-interactive. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <div
    class="stage"
    class:controlling
    class:picking={picker.active}
    bind:this={stage}
    role="application"
    aria-label={`Page view: ${alt}`}
    tabindex="0"
    data-testid="browser-stage"
    onfocus={() => (focused = true)}
    onblur={() => (focused = false)}
    onmousemove={onMouseMove}
    onmousedown={onMouseDown}
    onmouseup={onMouseUp}
    onclick={(event) => void onClick(event)}
    oncontextmenu={(event) => (control || picker.active) && event.preventDefault()}
    onkeydown={onKeyDown}
    onkeyup={onKeyUp}
  >
    <canvas bind:this={canvas} aria-label={alt} data-testid="browser-canvas">{alt}</canvas>
    {#if frame && meta}
      <PickOverlay {page} {picker} {frame} {meta} />
    {/if}
    {#if !meta}
      <p class="waiting">Waiting for the page to show…</p>
    {/if}
  </div>
  <PickList {page} />
</div>

<style>
  .page-view {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
    gap: 4px;
  }
  .tools {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
  }
  .mode,
  .waiting {
    color: #b4b4c0;
    font-size: 12px;
  }
  .mode.controlling {
    color: #ffd36b;
    font-weight: 600;
  }
  .stage {
    position: relative;
    flex: 1;
    min-height: 120px;
    overflow: hidden;
    background: #0c0c0f;
    border: 2px solid #2a2a31;
    border-radius: 3px;
  }
  .stage:focus-visible {
    outline: 2px solid #7aa2ff;
  }
  .stage.controlling {
    border-color: #ffd36b;
  }
  .stage.picking {
    cursor: crosshair;
  }
  canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }
  .waiting {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    margin: 0;
  }
  button {
    font: inherit;
    padding: 2px 10px;
    color: #e6e6e6;
    background: #2a2a33;
    border: 1px solid #4a4a57;
    border-radius: 3px;
    cursor: pointer;
  }
  button[aria-pressed="true"] {
    border-color: #7aa2ff;
    background: #33334a;
  }
  button:focus-visible {
    outline: 2px solid #7aa2ff;
    outline-offset: 1px;
  }
</style>
