<script lang="ts">
// One xterm.js instance for one terminal tab. It stays mounted while its tab
// is hidden, so scrollback survives tab and project switches.
import "@xterm/xterm/css/xterm.css";
import { FitAddon } from "@xterm/addon-fit";
import { type ISearchOptions, SearchAddon } from "@xterm/addon-search";
import { Terminal } from "@xterm/xterm";
import { tick, untrack } from "svelte";
import { inBrowserTab, isApple } from "../keys/shortcut";
import { projectFiles } from "../palette/files";
import { useOvra } from "../shell/context";
import { copySelection, pasteClipboard } from "./clipboard";
import FindBar from "./FindBar.svelte";
import { linkSpot } from "./file-links";
import { clipboardKey, isCommandKey, isFindKey } from "./keys";
import { fileLinkProvider, fileSet } from "./link-provider";
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

const { transport, workspace } = useOvra();
/** The project's root while it is the one shown; files of another project do not open. */
const root = () => (workspace.project?.id === session.project ? workspace.project.root : null);

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

/** Search highlights, from the theme tokens; the addon takes `#RRGGBB` only. */
function decorations(): ISearchOptions["decorations"] {
  const css = getComputedStyle(document.documentElement);
  const hex = (name: string, fallback: string) => {
    const value = css.getPropertyValue(name).trim();
    return /^#[0-9a-f]{6}$/i.test(value) ? value : fallback;
  };
  return {
    matchBackground: hex("--s4", "#2b3039"),
    matchOverviewRuler: hex("--text-3", "#6a727e"),
    activeMatchBackground: hex("--s4", "#2b3039"),
    activeMatchBorder: hex("--accent", "#5fbfdc"),
    activeMatchColorOverviewRuler: hex("--accent", "#5fbfdc"),
  };
}

let host: HTMLDivElement;
let term: Terminal | undefined = $state();
let fit: FitAddon | undefined;
let search: SearchAddon | undefined = $state();
let finding = $state(false);
let bar: ReturnType<typeof FindBar> | undefined = $state();

/** Shows the find bar, or focuses it when it is shown. */
export async function openFind(): Promise<void> {
  if (!search) return;
  finding = true;
  await tick();
  bar?.focus();
}

function closeFind(): void {
  finding = false;
  term?.focus();
}

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
      // The search addon highlights and counts matches with decorations, still a proposed API in xterm 6.
      allowProposedApi: true,
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
    const finder = new SearchAddon();
    t.loadAddon(finder);
    t.open(host);
    t.attachCustomKeyEventHandler((event) => {
      if (event.type !== "keydown") return true;
      if (leaves(event)) {
        event.preventDefault();
        onleave();
        return false;
      }
      // Prevented, so the window's Find in file or Search in files does not run as well.
      if (isFindKey(event, apple)) {
        event.preventDefault();
        void openFind();
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
      t.registerLinkProvider(
        fileLinkProvider({
          buffer: () => t.buffer.active,
          root,
          base: session.cwd ?? "",
          files: () => projectFiles(transport, session.project).then(fileSet),
          opens: (event) => (apple ? event.metaKey : event.ctrlKey),
          open: (link) => {
            if (root()) workspace.openFile(link.path, "edit", linkSpot(link));
          },
          hint: (text) => (text ? host.setAttribute("title", text) : host.removeAttribute("title")),
          hintText: `Open file (${apple ? "Cmd" : "Ctrl"}+click)`,
        }),
      ),
      t.onData((data) => sessions.input(session, data)),
      t.onResize(({ cols, rows }) => sessions.resize(session, { cols, rows })),
    ];
    const size = measure() ?? FALLBACK_SIZE;
    t.resize(size.cols, size.rows);
    const detach = session.attach({ write: (data) => t.write(data), reset: () => t.reset() });
    void sessions.start(session, size);
    term = t;
    search = finder;
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

{#if finding && search}
  <FindBar bind:this={bar} {search} decorations={decorations()} onclose={closeFind} />
{/if}
<div class="xterm-host" bind:this={host} data-testid="terminal-view"></div>

<style>
  .xterm-host {
    height: 100%;
    padding: 6px 0 0 14px;
    box-sizing: border-box;
    background: var(--s1);
  }
</style>
