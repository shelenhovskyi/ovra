// Where a screencast frame sits on the canvas, and how points and boxes move
// between the canvas and the page. Page coordinates are CSS pixels of the
// page's viewport, as `browser.input`, `browser.pick` and `Rect` use them.

import type { FrameMetadata, Rect } from "../lib/protocol";

/** The frame drawn into the canvas: its offset and CSS pixels per page pixel. */
export type Fit = { x: number; y: number; scale: number; width: number; height: number };

/** Scales a `page` viewport to fit `box` (both in CSS pixels), centred, never stretched. */
export function fitFrame(box: { width: number; height: number }, page: FrameMetadata): Fit {
  if (page.width <= 0 || page.height <= 0) return { x: 0, y: 0, scale: 1, width: 0, height: 0 };
  const scale = Math.min(box.width / page.width, box.height / page.height);
  const width = page.width * scale;
  const height = page.height * scale;
  return { x: (box.width - width) / 2, y: (box.height - height) / 2, scale, width, height };
}

/**
 * The page point under a canvas point (CSS pixels from the canvas's corner),
 * or null on the bars around the frame.
 */
export function toPage(at: { x: number; y: number }, frame: Fit): { x: number; y: number } | null {
  if (frame.scale <= 0) return null;
  const x = (at.x - frame.x) / frame.scale;
  const y = (at.y - frame.y) / frame.scale;
  if (x < 0 || y < 0 || x > frame.width / frame.scale || y > frame.height / frame.scale) return null;
  return { x: Math.round(x), y: Math.round(y) };
}

/** A page box on the canvas, in CSS pixels from the canvas's corner. */
export function toCanvas(rect: Rect, frame: Fit): Rect {
  return {
    x: frame.x + rect.x * frame.scale,
    y: frame.y + rect.y * frame.scale,
    width: rect.width * frame.scale,
    height: rect.height * frame.scale,
  };
}

/** `modifiers` of `browser.input`: Alt 1, Ctrl 2, Meta 4, Shift 8. */
export function modifiers(event: { altKey: boolean; ctrlKey: boolean; metaKey: boolean; shiftKey: boolean }): number {
  return (event.altKey ? 1 : 0) | (event.ctrlKey ? 2 : 0) | (event.metaKey ? 4 : 0) | (event.shiftKey ? 8 : 0);
}
