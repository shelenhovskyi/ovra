<script lang="ts">
// The terminal panel under the main area: one tab per shell of the open
// project. Mounted once by the shell; it owns the terminal commands. Hiding
// the panel keeps the shells and their scrollback.
import type { Terminal } from "@xterm/xterm";
import { tick } from "svelte";
import { registerCommand } from "../keys/commands";
import { formatShortcut, inBrowserTab, isApple, liveKeys } from "../keys/shortcut";
import { useOvra } from "../shell/context";
import Icon from "../shell/Icon.svelte";
import { handleOpenTerminalIn } from "./open-in";
import { loadScreenReader, saveScreenReader } from "./prefs";
import { exitNotice, LOST_NOTICE, type TerminalSession, TerminalSessions } from "./sessions.svelte";
import TerminalView from "./TerminalView.svelte";

const { transport, workspace } = useOvra();
const sessions = new TerminalSessions(transport);

const TOGGLE_KEYS = "Ctrl+`";
const NEW_KEYS = "Ctrl+Shift+`";

let open = $state(false);
let screenReader = $state(loadScreenReader());
/** Confirms a screen reader mode change to assistive technology. */
let readerNotice = $state("");
let panel: HTMLElement | undefined = $state();
const tabButtons: Record<number, HTMLButtonElement> = {};
const xterms = new Map<number, Terminal>();
/** Where focus was before the panel took it; it goes back there when the panel hides. */
let returnFocus: HTMLElement | null = null;

const project = $derived(workspace.project);
const tabs = $derived(project ? sessions.of(project.id) : []);
const active = $derived(project ? sessions.active(project.id) : undefined);

function label(keys: string): string {
  return formatShortcut(liveKeys(keys, inBrowserTab()), isApple());
}

function focusInPanel(): boolean {
  return !!panel && panel.contains(document.activeElement);
}

function rememberFocus(): void {
  if (focusInPanel()) return;
  returnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
}

async function focusActive(): Promise<void> {
  await tick();
  if (active) xterms.get(active.key)?.focus();
}

async function show(): Promise<void> {
  rememberFocus();
  open = true;
  if (project && tabs.length === 0) sessions.add(project.id);
  await focusActive();
}

function hide(): void {
  const hadFocus = focusInPanel();
  open = false;
  if (hadFocus && returnFocus?.isConnected) returnFocus.focus();
  returnFocus = null;
}

/** Opens a new tab, its shell in the project-relative folder `cwd` (the project root when left out). */
async function newTerminal(cwd?: string): Promise<void> {
  if (!project) {
    open = true;
    return;
  }
  rememberFocus();
  open = true;
  sessions.add(project.id, cwd);
  await focusActive();
}

async function close(session: TerminalSession, refocus: "tab" | "terminal"): Promise<void> {
  sessions.close(session);
  if (tabs.length === 0) {
    hide();
    return;
  }
  await tick();
  if (!active) return;
  if (refocus === "tab") tabButtons[active.key]?.focus();
  else xterms.get(active.key)?.focus();
}

async function select(session: TerminalSession): Promise<void> {
  sessions.activate(session);
  await tick();
  tabButtons[session.key]?.focus();
}

function onTabKeydown(event: KeyboardEvent, session: TerminalSession): void {
  const index = tabs.indexOf(session);
  const target = {
    ArrowRight: tabs[(index + 1) % tabs.length],
    ArrowLeft: tabs[(index - 1 + tabs.length) % tabs.length],
    Home: tabs[0],
    End: tabs[tabs.length - 1],
  }[event.key];
  if (target) {
    event.preventDefault();
    void select(target);
  } else if (event.key === "Delete") {
    event.preventDefault();
    void close(session, "tab");
  } else if (event.key === "Enter" || event.key === "ArrowDown") {
    event.preventDefault();
    xterms.get(session.key)?.focus();
  }
}

function setScreenReader(on: boolean): void {
  screenReader = on;
  saveScreenReader(on);
  readerNotice = on ? "Screen reader mode on: terminal output is read out." : "Screen reader mode off.";
}

/** Shift+Tab in a terminal lands on its tab, the start of the panel. */
function leave(session: TerminalSession): void {
  tabButtons[session.key]?.focus();
}

async function restart(session: TerminalSession): Promise<void> {
  sessions.restart(session);
  await focusActive();
}

function status(session: TerminalSession): string {
  switch (session.state.kind) {
    case "exited":
      return "exited";
    case "failed":
      return "failed";
    case "lost":
      return "gone";
    default:
      return "";
  }
}

$effect(() => sessions.listen());
$effect(() => handleOpenTerminalIn(newTerminal));

$effect(() => {
  const unregister = [
    registerCommand({
      id: "terminal.toggle",
      title: "Toggle terminal",
      keys: TOGGLE_KEYS,
      run: () => {
        if (!open) return show();
        if (focusInPanel()) return hide();
        return focusActive();
      },
    }),
    registerCommand({ id: "terminal.new", title: "New terminal", keys: NEW_KEYS, run: () => newTerminal() }),
    registerCommand({
      id: "terminal.close",
      title: "Close terminal",
      run: () => (active && open ? close(active, "terminal") : undefined),
    }),
    registerCommand({
      id: "terminal.screen_reader",
      title: "Toggle screen reader mode in terminals",
      run: () => setScreenReader(!screenReader),
    }),
  ];
  return () => {
    for (const off of unregister) off();
  };
});
</script>

<section class="terminals" aria-label="Terminal panel" hidden={!open} bind:this={panel} data-testid="terminals">
  <div class="bar">
    <div class="tabs" role="tablist" aria-label="Terminals">
      {#each tabs as session (session.key)}
        {@const selected = session === active}
        {@const note = status(session)}
        <div class="tab" class:selected>
          <button
            type="button"
            role="tab"
            id="terminal-tab-{session.key}"
            aria-selected={selected}
            aria-controls="terminal-panel-{session.key}"
            tabindex={selected ? 0 : -1}
            bind:this={tabButtons[session.key]}
            onclick={() => select(session)}
            onkeydown={(event) => onTabKeydown(event, session)}
          >
            {session.title}{#if note}<span class="note">{` (${note})`}</span>{/if}
          </button>
          <button
            type="button"
            class="close"
            tabindex="-1"
            aria-label="Close {session.title}"
            title="Close {session.title} (Delete)"
            onclick={() => close(session, "tab")}><Icon name="close" small /></button
          >
        </div>
      {/each}
    </div>
    <div class="actions">
      <button
        type="button"
        disabled={!project}
        aria-label="New terminal"
        title="New terminal ({label(NEW_KEYS)})"
        onclick={() => newTerminal()}>+</button
      >
      <button
        type="button"
        class="reader"
        aria-pressed={screenReader}
        aria-label="Screen reader"
        title="Read terminal output to a screen reader"
        onclick={() => setScreenReader(!screenReader)}
        ><Icon name="speaker" small /></button
      >
      <button
        type="button"
        class="hide"
        aria-label="Hide terminals"
        title="Hide terminals ({label(TOGGLE_KEYS)})"
        onclick={hide}
        ><Icon name="chevron" small /></button
      >
    </div>
  </div>
  <p id="terminal-hint" class="sr-only">Shift+Tab leaves the terminal. {label(TOGGLE_KEYS)} hides the panel.</p>
  <p class="sr-only" aria-live="polite">{readerNotice}</p>
  <div class="body">
    {#if !project}
      <p class="empty">No project open.</p>
    {:else if tabs.length === 0}
      <p class="empty">
        No terminals.
        <button type="button" class="link" onclick={() => newTerminal()}>New terminal</button>
        ({label(NEW_KEYS)})
      </p>
    {/if}
    {#each sessions.sessions as session (session.key)}
      {@const shown = session === active && session.project === project?.id}
      <div
        class="panel"
        role="tabpanel"
        id="terminal-panel-{session.key}"
        aria-labelledby="terminal-tab-{session.key}"
        hidden={!shown}
      >
        {#if session.state.kind === "failed"}
          <p class="failed" role="alert">Could not start the terminal: {session.state.message}</p>
        {:else if session.state.kind === "exited" || session.state.kind === "lost"}
          <div class="ended">
            <p role="status">
              {session.title}: {session.state.kind === "exited" ? exitNotice(session.state.code) : LOST_NOTICE}
            </p>
            <button type="button" class="link" onclick={() => restart(session)}>Restart</button>
          </div>
        {/if}
        <TerminalView
          {session}
          {sessions}
          {screenReader}
          hint="terminal-hint"
          onleave={() => leave(session)}
          visible={open && shown}
          onterminal={(terminal) => {
            if (terminal) xterms.set(session.key, terminal);
            else xterms.delete(session.key);
          }}
        />
      </div>
    {/each}
  </div>
</section>

<style>
  .terminals {
    display: flex;
    flex-direction: column;
    flex: 0 0 40%;
    min-height: 120px;
    background: var(--s2);
  }
  .terminals[hidden] {
    display: none;
  }
  .bar {
    display: flex;
    align-items: stretch;
    padding: 4px 8px 4px 10px;
    background: var(--s2);
  }
  .tabs {
    display: flex;
    flex: 1;
    gap: 2px;
    overflow-x: auto;
    scrollbar-width: thin;
  }
  /* Like the editor's tabs: tone, not lines; the selected one is a --s3 pill. */
  .tab {
    display: flex;
    align-items: center;
    flex-shrink: 0;
    border-radius: var(--radius-sm);
    color: var(--text-2);
  }
  .tab:hover {
    color: var(--text);
  }
  .tab.selected {
    color: var(--text);
    font-weight: 500;
    background: var(--s3);
  }
  button {
    font: inherit;
    color: inherit;
    background: none;
    border: none;
    cursor: pointer;
  }
  button:disabled {
    cursor: default;
    opacity: 0.5;
  }
  button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
  [role="tab"] {
    padding: 5px 2px 5px 10px;
  }
  .note {
    font-weight: 400;
    color: var(--text-2);
  }
  .close {
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    margin-right: 4px;
    border-radius: var(--radius-sm);
    color: var(--text-2);
    opacity: 0;
  }
  .tab.selected .close,
  .tab:hover .close,
  .close:focus-visible {
    opacity: 1;
  }
  .close:hover {
    background: var(--s4);
    color: var(--text);
  }
  .actions button:hover:not(:disabled) {
    background: var(--s3);
    color: var(--text);
  }
  /* Secondary chrome: muted until the panel is pointed at or a button is focused. */
  .actions {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 0 2px;
  }
  .actions button {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border-radius: var(--radius-sm);
    font-size: 16px;
    color: var(--text-3);
  }
  .terminals:hover .actions button,
  .actions button:focus-visible {
    color: var(--text-2);
  }
  .actions .reader[aria-pressed="true"] {
    background: var(--accent-soft);
    color: var(--accent);
  }
  .ended {
    display: flex;
    align-items: baseline;
    gap: 12px;
    margin: 4px 10px;
    padding: 6px 12px;
    border-radius: var(--radius);
    background: var(--s3);
    color: var(--text-2);
  }
  .ended p {
    margin: 0;
  }
  .body {
    position: relative;
    flex: 1;
    min-height: 0;
  }
  .panel {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
  }
  .panel[hidden] {
    display: none;
  }
  .panel > :global(.xterm-host) {
    flex: 1;
    min-height: 0;
  }
  .empty,
  .failed {
    margin: 0;
    padding: 10px 16px;
  }
  .empty {
    color: var(--text-2);
  }
  .link {
    padding: 0;
    color: var(--accent);
    text-decoration: underline;
  }
  .failed {
    color: var(--danger);
  }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
  @media (forced-colors: active) {
    .close {
      opacity: 1;
    }
    .actions .reader[aria-pressed="true"] {
      outline: 2px solid Highlight;
      outline-offset: -2px;
    }
    .tab.selected {
      forced-color-adjust: none;
      background: Highlight;
      color: HighlightText;
    }
  }
</style>
