<script lang="ts">
// One xterm.js instance for one terminal tab. It stays mounted while its tab
// is hidden, so scrollback survives tab and project switches.
import "@xterm/xterm/css/xterm.css";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import { untrack } from "svelte";
import { inBrowserTab, isApple } from "../keys/shortcut";
import { isCommandKey } from "./keys";
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
      fontFamily: 'ui-monospace, "SF Mono", Menlo, Consolas, "Liberation Mono", monospace',
      fontSize: 13,
      scrollback: 5000,
      screenReaderMode: screenReader,
      theme: { background: "#16161a", foreground: "#e6e6e6", cursor: "#e6e6e6", selectionBackground: "#3a3a46" },
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
    return () => {
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
    padding: 4px 0 0 8px;
    box-sizing: border-box;
  }
</style>
