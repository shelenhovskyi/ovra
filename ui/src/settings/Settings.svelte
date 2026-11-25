<script lang="ts">
// The Settings view in the main area (Mod+, or the rail's gear): a list of
// panes in two groups, the open project's and Ovra's, and the chosen pane
// beside it. Each pane has a noun for a title, one line on what it does and
// one column of fields. Arrow keys move in the list.
import { untrack } from "svelte";
import { listCommands } from "../keys/commands";
import { formatShortcut, inBrowserTab, isApple, liveKeys } from "../keys/shortcut";
import type { CoreInfo } from "../lib/protocol";
import NetworkPanel from "../network/NetworkPanel.svelte";
import PolicyPanel from "../policy/PolicyPanel.svelte";
import ServersPanel from "../servers/ServersPanel.svelte";
import { useOvra } from "../shell/context";
import ViewFrame from "../shell/ViewFrame.svelte";
import { loadScreenReader } from "../terminal/prefs";
import { type PaneId, panesFor, settings } from "./panes.svelte";

const { transport, workspace } = useOvra();

const GROUPS = [
  { id: "project", name: "This project" },
  { id: "ovra", name: "Ovra" },
] as const;

const panes = $derived(panesFor(!!workspace.project));
/** The remembered pane, or the first there is (a project's pane with no project open). */
const pane = $derived(panes.find((p) => p.id === settings.pane) ?? panes[0]);

let info = $state<CoreInfo | null>(null);
let screenReader = $state(loadScreenReader());
let permission = $state(notificationPermission());

$effect(() =>
  untrack(() => {
    transport
      .call({ method: "info.get" })
      .then((reply) => (info = reply.data))
      .catch(() => (info = null));
  }),
);

const shortcuts = $derived.by(() => {
  void pane;
  return listCommands().flatMap((c) =>
    c.keys ? [{ id: c.id, title: c.title, keys: formatShortcut(liveKeys(c.keys, inBrowserTab()), isApple()) }] : [],
  );
});

function notificationPermission(): NotificationPermission | "unsupported" {
  return typeof Notification === "undefined" ? "unsupported" : Notification.permission;
}

async function run(id: string): Promise<void> {
  await listCommands()
    .find((c) => c.id === id)
    ?.run();
  screenReader = loadScreenReader();
  permission = notificationPermission();
}

function choose(id: PaneId): void {
  settings.pane = id;
}

function onListKeydown(event: KeyboardEvent): void {
  const moves: Record<string, number> = { ArrowDown: 1, ArrowUp: -1 };
  const at = panes.findIndex((p) => p.id === pane?.id);
  const next =
    event.code === "Home"
      ? panes[0]
      : event.code === "End"
        ? panes.at(-1)
        : event.code in moves
          ? panes[Math.max(0, Math.min(panes.length - 1, at + (moves[event.code] ?? 0)))]
          : undefined;
  if (!next) return;
  event.preventDefault();
  choose(next.id);
  (event.currentTarget as HTMLElement).querySelector<HTMLElement>(`[data-pane=${next.id}]`)?.focus();
}
</script>

<ViewFrame name="settings" testid="settings">
  <div class="settings">
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <nav aria-label="Settings panes" onkeydown={onListKeydown}>
      {#each GROUPS as group (group.id)}
        {@const own = panes.filter((p) => p.group === group.id)}
        {#if own.length > 0}
          <h3 class="group">{group.name}</h3>
          <ul>
            {#each own as item (item.id)}
              <li>
                <button
                  type="button"
                  data-pane={item.id}
                  aria-current={item.id === pane?.id ? "page" : undefined}
                  tabindex={item.id === pane?.id ? 0 : -1}
                  onclick={() => choose(item.id)}>{item.title}</button
                >
              </li>
            {/each}
          </ul>
        {/if}
      {/each}
    </nav>
    {#if pane}
      <section class="pane" aria-labelledby="settings-pane-title" data-testid={`settings-${pane.id}`}>
        <h3 id="settings-pane-title" class="title">{pane.title}</h3>
        <p class="line">{pane.line}</p>
        {#if pane.id === "rules"}
          <PolicyPanel />
        {:else if pane.id === "network"}
          <NetworkPanel />
        {:else if pane.id === "servers"}
          <ServersPanel />
        {:else if pane.id === "notifications"}
          <p class="field">
            <span>System notifications</span>
            <strong data-testid="notifications-state"
              >{{ granted: "On", denied: "Blocked", default: "Off", unsupported: "Not available here" }[permission]}</strong
            >
          </p>
          {#if permission === "default"}
            <button type="button" class="primary" onclick={() => void run("notify.enable_system")}>Turn on</button>
          {:else if permission === "denied"}
            <p class="note">Allow them for Ovra in your system's settings.</p>
          {/if}
          <p class="note">Shown only while Ovra is in the background. Inside the window, toasts say the same.</p>
        {:else if pane.id === "keyboard"}
          <table aria-label="Keyboard shortcuts">
            <tbody>
              {#each shortcuts as shortcut (shortcut.id)}
                <tr><td>{shortcut.title}</td><td><kbd>{shortcut.keys}</kbd></td></tr>
              {/each}
            </tbody>
          </table>
        {:else if pane.id === "accessibility"}
          <label class="check">
            <input
              type="checkbox"
              checked={screenReader}
              onchange={() => void run("terminal.screen_reader")}
              data-testid="screen-reader"
            />
            Read terminal output to a screen reader
          </label>
          <p class="note">Every shortcut has a command in Search or run, and every panel can be reached with Tab.</p>
        {:else if pane.id === "about"}
          <p class="field">
            <span>Version</span><strong data-testid="about-version">{info ? `${info.name} ${info.version}` : "…"}</strong>
          </p>
          {#if info}<p class="field"><span>Mode</span><strong>{info.mode}</strong></p>{/if}
          <p class="note">Icons from Lucide, under the ISC license.</p>
        {/if}
      </section>
    {/if}
  </div>
</ViewFrame>

<style>
  .settings {
    display: grid;
    grid-template-columns: minmax(160px, 200px) minmax(0, 1fr);
    flex: 1;
    min-height: 0;
  }
  nav {
    padding: var(--sp-2) var(--sp-2) var(--sp-4) var(--sp-3);
    overflow-y: auto;
  }
  .group {
    margin: var(--sp-3) 0 var(--sp-1) var(--sp-2);
    font: var(--t-caption);
    font-weight: 600;
    color: var(--text-2);
  }
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  nav button {
    width: 100%;
    height: var(--row);
    padding: 0 var(--sp-2);
    font: var(--t-body);
    text-align: left;
    color: var(--text);
    background: transparent;
    border: 0;
    border-radius: var(--r-sm);
    cursor: default;
  }
  nav button:hover {
    background: var(--s3);
  }
  nav button[aria-current="page"] {
    font-weight: 500;
    background: var(--sel-focus);
  }
  .pane {
    max-width: 640px;
    padding: var(--sp-3) var(--sp-5) var(--sp-6);
    overflow-y: auto;
  }
  .title {
    margin: 0;
    font: var(--t-title2);
    letter-spacing: -0.02em;
  }
  .line {
    margin: var(--sp-1) 0 var(--sp-4);
    color: var(--text-2);
  }
  .field {
    display: flex;
    gap: var(--sp-3);
    max-width: 480px;
    margin: 0 0 var(--sp-2);
  }
  .field span {
    min-width: 160px;
    color: var(--text-2);
  }
  .field strong {
    font-weight: 500;
  }
  .note {
    margin: var(--sp-3) 0 0;
    font: var(--t-small);
    color: var(--text-2);
  }
  .check {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
  }
  button.primary {
    height: var(--ctl);
    padding: 0 var(--sp-3);
    font: inherit;
    font-weight: 500;
    color: var(--accent-ink);
    background: var(--accent);
    border: 0;
    border-radius: var(--r-md);
    cursor: default;
  }
  table {
    width: 100%;
    max-width: 480px;
    border-collapse: collapse;
  }
  td {
    height: var(--row);
    padding: 0 var(--sp-2) 0 0;
  }
  td + td {
    text-align: right;
  }
  kbd {
    font: var(--t-caption);
    font-variant-numeric: tabular-nums;
    color: var(--text-2);
  }
  @media (forced-colors: active) {
    nav button[aria-current="page"] {
      outline: 2px solid Highlight;
      outline-offset: -2px;
    }
  }
</style>
