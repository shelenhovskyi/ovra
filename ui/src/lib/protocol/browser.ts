// Mirrors crates/core/src/protocol/browser.rs: the agent browser.

import type { SessionId } from "./agents";

export type PageId = string;

export type PageInfo = { id: PageId; session: SessionId; url: string; title: string };

/** CSS pixels of the page's viewport, rounded. */
export type Rect = { x: number; y: number; width: number; height: number };

export type MouseButton = "left" | "middle" | "right";

/**
 * The person's input for a page they took over. `modifiers` is a bit set:
 * Alt 1, Ctrl 2, Meta 4, Shift 8.
 */
export type InputEvent =
  | { kind: "mouse_move"; x: number; y: number; modifiers?: number }
  | { kind: "mouse_down"; x: number; y: number; button: MouseButton; clicks: number; modifiers?: number }
  | { kind: "mouse_up"; x: number; y: number; button: MouseButton; clicks: number; modifiers?: number }
  | { kind: "wheel"; x: number; y: number; delta_x: number; delta_y: number; modifiers?: number }
  | { kind: "key_down"; key: string; code: string; text?: string | null; modifiers?: number }
  | { kind: "key_up"; key: string; code: string; modifiers?: number };

/** Read from what the page's build exposes; always a guess. */
export type SourceGuess = { component: string | null; file: string | null; line: number | null };

/** An element the person picked, as the agent gets it. */
export type Pick = {
  pick_id: string;
  url: string;
  selector: string;
  /** A role-and-name locator, e.g. `button "Save"`. */
  locator: string;
  tag: string;
  id?: string | null;
  classes?: string[];
  attributes?: Record<string, string>;
  /** The first 200 characters of its text. */
  text: string;
  /** `outerHTML`, cut to 2 KiB. */
  html: string;
  rect: Rect;
  styles?: Record<string, string>;
  source?: SourceGuess | null;
  note?: string;
};

/** What a screencast frame shows, to map the canvas onto the page. */
export type FrameMetadata = { width: number; height: number; scroll_x: number; scroll_y: number };

/** Why a session's agent browser cannot run. */
export type BrowserRefusal = "no_chromium" | "no_sandbox" | "not_supported" | "not_sandboxed" | "session_ended";

export type BrowserAvailability =
  | { state: "available" }
  /** `reason` is the core's words, with the detail the code lacks. */
  | { state: "refused"; code: BrowserRefusal; reason: string };

export type BrowserRequest =
  /** Whether the session's browser can run, without starting it. */
  | { method: "browser.status"; params: { session: SessionId } }
  | {
      method: "browser.open";
      params: { session: SessionId; url: string; width?: number | null; height?: number | null };
    }
  | { method: "browser.close"; params: { page: PageId } }
  /** Frames flow only while a panel shows the page. */
  | { method: "browser.screencast"; params: { page: PageId; on: boolean } }
  | { method: "browser.input"; params: { page: PageId; event: InputEvent } }
  /** The element at a point, or its ancestor `up` levels above. */
  | { method: "browser.pick"; params: { page: PageId; x: number; y: number; up?: number } }
  /** The picks of a message the person sends, with their notes. */
  | { method: "browser.attach"; params: { page: PageId; picks: Pick[] } }
  | { method: "browser.answer_navigation"; params: { page: PageId; origin: string; allow: boolean } };

export type BrowserResponse =
  | { type: "browser.page"; data: PageInfo }
  | { type: "browser.picked"; data: Pick }
  | { type: "browser.status"; data: BrowserAvailability };

export type BrowserEvent =
  /** `jpeg` is base64. */
  | { type: "browser.frame"; data: { page: PageId; jpeg: string; metadata: FrameMetadata } }
  | { type: "browser.pages"; data: { session: SessionId; pages: PageInfo[] } }
  | { type: "browser.navigation_asked"; data: { page: PageId; session: SessionId; origin: string } };

/** The response type of each `browser.*` method. */
export type BrowserResponseFor<M extends BrowserRequest["method"]> = M extends "browser.open"
  ? Extract<BrowserResponse, { type: "browser.page" }>
  : M extends "browser.pick"
    ? Extract<BrowserResponse, { type: "browser.picked" }>
    : M extends "browser.status"
      ? Extract<BrowserResponse, { type: "browser.status" }>
      : { type: "ok" };
