<script lang="ts">
// One page of a session's browser: its screencast on a canvas, scaled to fit
// and asked for only while the canvas can be seen.
import { untrack } from "svelte";
import type { FrameMetadata, PageInfo } from "../lib/protocol";
import { useOvra } from "../shell/context";
import { sessionBrowsers } from "./browser.svelte";
import { fitFrame } from "./geometry";

let { page }: { page: PageInfo } = $props();

const { transport } = useOvra();
const browsers = sessionBrowsers(transport);

let stage = $state<HTMLElement>();
let canvas = $state<HTMLCanvasElement>();
let box = $state({ width: 0, height: 0 });
let meta = $state<FrameMetadata | null>(null);
let intersecting = $state(typeof IntersectionObserver === "undefined");
let tabVisible = $state(document.visibilityState !== "hidden");

const frame = $derived(meta ? fitFrame(box, meta) : null);
const alt = $derived(`${page.title || "Untitled page"}, ${page.url}`);

// Frames: decode the newest one, skip any that arrived meanwhile.
let next: { jpeg: string; metadata: FrameMetadata } | null = null;
let decoding = false;
let bitmap: ImageBitmap | null = null;

$effect(() =>
  transport.onEvent((event) => {
    if (event.type !== "browser.frame" || event.data.page !== page.id) return;
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
  const params = { page: page.id, on };
  browsers.guard(() => transport.call({ method: "browser.screencast", params })).catch(() => {});
}

$effect(() => {
  const on = intersecting && tabVisible;
  untrack(() => screencast(on));
});
$effect(() => () => untrack(() => screencast(false)));
</script>

<div class="page-view">
  <div class="stage" bind:this={stage} data-testid="browser-stage">
    <canvas bind:this={canvas} aria-label={alt} data-testid="browser-canvas">{alt}</canvas>
    {#if !meta}
      <p class="waiting">Waiting for the page to show…</p>
    {/if}
  </div>
</div>

<style>
  .page-view {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
    gap: 4px;
  }
  .waiting {
    color: #b4b4c0;
    font-size: 12px;
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
</style>
