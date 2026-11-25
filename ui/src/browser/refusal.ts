// What the pane says when a session gets no browser: a sentence for each
// refusal code of `browser.status`, the core's words as the detail where
// they add something, and whether trying again could help.

import type { BrowserRefusal } from "../lib/protocol";

/** Why a session's browser cannot run; `code` is null when a call was refused without one. */
export type Refusal = { code: BrowserRefusal | null; reason: string };

export type RefusalText = { headline: string; detail: string | null; retry: boolean };

/** The core's words as a sentence. */
export function sentence(reason: string): string {
  const text = reason.trim();
  return `${text.charAt(0).toUpperCase()}${text.slice(1)}${/[.!?]$/.test(text) ? "" : "."}`;
}

export function refusalText({ code, reason }: Refusal): RefusalText {
  const detail = sentence(reason);
  switch (code) {
    case "no_chromium":
      return {
        headline: "The agent browser needs Chromium or Google Chrome. Install one, then restart Ovra.",
        detail,
        retry: false,
      };
    case "no_sandbox":
      return {
        headline: "The agent browser runs only in the sandbox, which this computer lacks.",
        detail,
        retry: false,
      };
    case "not_supported":
      return { headline: "The agent browser works only on Linux for now.", detail: null, retry: false };
    case "not_sandboxed":
      return {
        headline: "This agent runs outside the sandbox, so it gets no browser.",
        detail: "Agents that run in the sandbox each get their own.",
        retry: false,
      };
    case "session_ended":
      return { headline: "This agent has ended, and its browser with it.", detail: null, retry: false };
    case null:
      return { headline: detail, detail: null, retry: true };
  }
}
