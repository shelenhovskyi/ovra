<script lang="ts">
// One xterm.js instance for one terminal tab. It stays mounted while its tab
// is hidden, so scrollback survives tab and project switches.
import "@xterm/xterm/css/xterm.css";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import { untrack } from "svelte";
import { inBrowserTab, isApple } from "../keys/shortcut";
import { copySelection, pasteClipboard } from "./clipboard";
import { clipboardKey, isCommandKey } from "./keys";
import type { TerminalSession, TerminalSessions } from "./sessions.svelte";

let {
  session,
  sessions,
  visible,
  screenReader,
  hint,
  onterminal,
  onleave,
}: {
  session: TerminalSession;
  sessions: TerminalSessions;
  visible: boolean;
  screenReader: boolean;
  /** Id of the text that says how to leave the terminal. */
  hint: string;
  /** Shift+Tab: the panel moves focus out of the terminal. */
  onleave: () => void;
  /** Hands the xterm instance to the panel, which focuses it. */
  onterminal: (terminal: Terminal | null) => void;
} = $props();

/** Shift+Tab is the documented way out of the terminal (WCAG 2.1.2); the shell rarely needs it. */
function leaves(event: KeyboardEvent): boolean {
  return event.key === "Tab" && event.shiftKey && !event.ctrlKey && !event.altKey && !event.metaKey;
}

const FALLBACK_SIZE = { cols: 80, rows: 24 };

/** xterm draws its own colours, so it gets the theme tokens' values, not `var()`. */
function colours(): { background: string; foreground: string; cursor: string; selectionBackground: string } {
  const css = getComputedStyle(document.documentElement);
  const token = (name: string, fallback: string) => css.getPropertyValue(name).trim() || fallback;
  return {
    background: token("--s1", "black"),
    foreground: token("--text", "white"),
    cursor: token("--text", "white"),
    selectionBackground: token("--s4", "gray"),
  };
}

let host: HTMLDivElement;
let term: Terminal | undefined = $state();
let fit: FitAddon | undefined;

/** Fits the terminal to its box; null while the box is hidden or empty. */
function measure(): { cols: number; rows: number } | null {
  const dims = fit?.proposeDimensions();
  if (!dims || !Number.isFinite(dims.cols) || !Number.isFinite(dims.rows) || dims.cols < 1 || dims.rows < 1) {
    return null;
  }
  return { cols: dims.cols, rows: dims.rows };
}

function refit(): void {
  if (!term || !visible) return;
  const size = measure();
  if (size && (size.cols !== term.cols || size.rows !== term.rows)) term.resize(size.cols, size.rows);
}

$effect(() =>
  untrack(() => {
    const apple = isApple();
    const browser = inBrowserTab();
    const t = new Terminal({
      cursorBlink: true,
      fontFamily:
        getComputedStyle(document.documentElement).getPropertyValue("--mono").trim() ||
        'ui-monospace, "SF Mono", Menlo, Consolas, "Liberation Mono", monospace',
      fontSize: 13,
      lineHeight: 1.2,
      scrollback: 5000,
      screenReaderMode: screenReader,
      theme: colours(),
    });
    fit = new FitAddon();
    t.loadAddon(fit);
    t.open(host);
    t.attachCustomKeyEventHandler((event) => {
      if (event.type !== "keydown") return true;
      if (leaves(event)) {
        event.preventDefault();
        onleave();
        return false;
      }
      const clip = clipboardKey(event, apple);
      // Not prevented: the browser's own paste follows and xterm takes it, no clipboard read needed.
      if (clip === "system-paste") return false;
      // Off Apple keyboards the copy key never reaches the shell, also with nothing selected.
      if (clip === "paste" || (clip === "copy" && (copySelection(t) || !apple))) {
        event.preventDefault();
        if (clip === "paste") pasteClipboard(t);
        return false;
      }
      // Returning false leaves the key to the window, where the command runs.
      return !isCommandKey(event, apple, browser);
    });
    t.textarea?.setAttribute("aria-describedby", hint);
    const subs = [
      t.onData((data) => sessions.input(session, data)),
      t.onResize(({ cols, rows }) => sessions.resize(session, { cols, rows })),
    ];
    const size = measure() ?? FALLBACK_SIZE;
    t.resize(size.cols, size.rows);
    const detach = session.attach({ write: (data) => t.write(data), reset: () => t.reset() });
    void sessions.start(session, size);
    term = t;
    onterminal(t);

    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(() => refit());
    observer?.observe(host);
    // The tokens change with the system's light or dark theme.
    const scheme = typeof matchMedia === "function" ? matchMedia("(prefers-color-scheme: light)") : null;
    const recolour = () => (t.options.theme = colours());
    scheme?.addEventListener("change", recolour);
    return () => {
      scheme?.removeEventListener("change", recolour);
      observer?.disconnect();
      detach();
      for (const sub of subs) sub.dispose();
      onterminal(null);
      t.dispose();
    };
  }),
);

$effect(() => {
  if (visible) untrack(refit);
});

$effect(() => {
  if (term) term.options.disableStdin = session.state.kind !== "running" && session.state.kind !== "starting";
});

$effect(() => {
  if (term) term.options.screenReaderMode = screenReader;
});
</script>

<div class="xterm-host" bind:this={host} data-testid="terminal-view"></div>

<style>
  .xterm-host {
    height: 100%;
    padding: 6px 0 0 14px;
    box-sizing: border-box;
    background: var(--s1);
  }
</style>
