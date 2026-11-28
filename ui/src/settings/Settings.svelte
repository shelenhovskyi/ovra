<script lang="ts">
// The Settings window over the app (Mod+, or the rail's gear), laid out like
// macOS System Settings: a search field and a list of panes in two groups, the
// open project's and Ovra's, each with an icon on a tinted tile; the chosen
// pane beside them. Each pane has a noun for a title, one line on what it does
// and one column of fields. Arrow keys move in the list; Escape or a click
// outside closes the window.
import { untrack } from "svelte";
import { providers } from "../agents/providers.svelte";
import { listCommands } from "../keys/commands";
import { listContextKeys } from "../keys/context-keys";
import { formatShortcut, inBrowserTab, isApple, liveKeys } from "../keys/shortcut";
import type { CoreInfo } from "../lib/protocol";
import MachinesPane from "../machines/MachinesPane.svelte";
import KeysPane from "../network/KeysPane.svelte";
import NetworkPanel from "../network/NetworkPanel.svelte";
import PolicyPanel from "../policy/PolicyPanel.svelte";
import KeepAwakeField from "../power/KeepAwakeField.svelte";
import PhonePane from "../remote/PhonePane.svelte";
import SchedulesPane from "../schedules/SchedulesPane.svelte";
import SchedulesSwitch from "../schedules/SchedulesSwitch.svelte";
import { schedules } from "../schedules/schedules.svelte";
import ServersPanel from "../servers/ServersPanel.svelte";
import { useOvra } from "../shell/context";
import Icon from "../shell/Icon.svelte";
import { loadScreenReader } from "../terminal/prefs";
import AgentLimitField from "./AgentLimitField.svelte";
import AgentsPane from "./AgentsPane.svelte";
import AttemptsField from "./AttemptsField.svelte";
import EditorPane from "./EditorPane.svelte";
import ModelField from "./ModelField.svelte";
import MultiRepoField from "./MultiRepoField.svelte";
import PlanFirstField from "./PlanFirstField.svelte";
import ProviderCard from "./ProviderCard.svelte";
import { findPanes, type PaneId, panesFor, settings } from "./panes.svelte";
import ResourcesField from "./ResourcesField.svelte";
import SetupPane from "./SetupPane.svelte";
import TemplatesField from "./TemplatesField.svelte";
import TrustPane from "./TrustPane.svelte";
import { settingsWindow } from "./window.svelte";

const { transport, workspace } = useOvra();
const names = providers(transport);
/** The Keyboard pane's group for the commands' shortcuts. */
const EVERYWHERE = "Everywhere";

// The project panes change one project: its name says which.
const GROUPS = $derived([
  { id: "project", name: `This project · ${workspace.project?.name ?? ""}` },
  { id: "ovra", name: "Ovra" },
] as const);

const scheduled = schedules(transport);
const panes = $derived(panesFor(!!workspace.project, scheduled.on === true));
/** What the search field holds; while it holds words, the list shows the panes that match. */
let query = $state("");
const found = $derived(query.trim() ? findPanes(panes, query) : null);
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

/** What the Keyboard pane's filter keeps: words of a title, a key, or a place. */
let keyFilter = $state("");

/**
 * Every key, grouped by where it works: the commands' shortcuts work
 * everywhere unless they name a place, then the keys of one place, by place.
 */
const keyGroups = $derived.by(() => {
  void pane;
  const apple = isApple();
  const rows = [
    ...listCommands().flatMap((c) =>
      c.keys
        ? [
            {
              id: c.id,
              title: c.title,
              binding: c.keys,
              keys: formatShortcut(liveKeys(c.keys, inBrowserTab()), apple),
              where: c.where ?? EVERYWHERE,
            },
          ]
        : [],
    ),
    ...listContextKeys().map((k, i) => ({
      id: `${k.where}:${i}`,
      title: k.title,
      binding: k.keys,
      keys: formatShortcut(k.keys, apple),
      where: k.where,
    })),
  ];
  const wanted = keyFilter.trim().toLowerCase();
  const kept = wanted
    ? rows.filter((r) => [r.title, r.keys, r.where].some((text) => text.toLowerCase().includes(wanted)))
    : rows;
  const places = [...new Set(kept.map((r) => r.where))].sort((a, b) =>
    a === EVERYWHERE ? -1 : b === EVERYWHERE ? 1 : a.localeCompare(b),
  );
  return places.map((where) => ({ where, rows: kept.filter((r) => r.where === where) }));
});

/** Escape in a filter with text clears it; the next one closes Settings. */
function onFilterKeydown(event: KeyboardEvent): void {
  if (event.code !== "Escape" || !keyFilter) return;
  event.preventDefault();
  keyFilter = "";
}

/** In the search field: Escape clears it first, Return opens the first pane found, Down moves into the list. */
function onSearchKeydown(event: KeyboardEvent): void {
  if (event.code === "Escape" && query) {
    event.preventDefault();
    query = "";
  } else if (event.code === "Enter" && found?.[0]) {
    event.preventDefault();
    choose(found[0].id);
  } else if (event.code === "ArrowDown") {
    event.preventDefault();
    listBox?.querySelector<HTMLElement>("button")?.focus();
  }
}

let windowBox = $state<HTMLElement>();
let listBox = $state<HTMLElement>();

/** Escape closes the window; Tab stays inside it. */
function onWindowKeydown(event: KeyboardEvent): void {
  if (event.code === "Escape" && !event.defaultPrevented) {
    event.preventDefault();
    settingsWindow.close();
  } else if (event.code === "Tab" && windowBox) {
    const stops = [
      ...windowBox.querySelectorAll<HTMLElement>(
        "button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), a[href], [tabindex='0']",
      ),
    ].filter((el) => el.offsetParent !== null || el === document.activeElement);
    const first = stops[0];
    const last = stops.at(-1);
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last?.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first?.focus();
    }
  }
}

/** The search field takes the focus when the window opens, so typing finds a setting. */
function focusSearch(field: HTMLInputElement): void {
  field.focus();
}

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
  const shown = found ?? panes;
  const at = shown.findIndex((p) => p.id === (event.target as HTMLElement).dataset.pane);
  const next =
    event.code === "Home"
      ? shown[0]
      : event.code === "End"
        ? shown.at(-1)
        : event.code in moves
          ? shown[Math.max(0, Math.min(shown.length - 1, at + (moves[event.code] ?? 0)))]
          : undefined;
  if (!next) return;
  event.preventDefault();
  choose(next.id);
  (event.currentTarget as HTMLElement).querySelector<HTMLElement>(`[data-pane=${next.id}]`)?.focus();
}
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="scrim" onclick={() => settingsWindow.close()}></div>
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="window"
  role="dialog"
  aria-modal="true"
  aria-label="Settings"
  data-commands="settings. palette."
  data-testid="settings"
  tabindex="-1"
  bind:this={windowBox}
  onkeydown={onWindowKeydown}
>
  <aside>
    <button
      type="button"
      class="close"
      aria-label="Close settings"
      title="Close (Escape)"
      aria-keyshortcuts="Escape"
      onclick={() => settingsWindow.close()}><Icon name="close" small /></button
    >
    <div class="find"><Icon name="search" small /><input
      type="search"
      class="search"
      bind:value={query}
      placeholder="Search"
      aria-label="Search settings"
      aria-controls="settings-panes"
      autocomplete="off"
      spellcheck="false"
      onkeydown={onSearchKeydown}
      {@attach focusSearch}
      data-testid="settings-search"
    /></div>
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <nav id="settings-panes" aria-label="Settings panes" onkeydown={onListKeydown} bind:this={listBox}>
      {#snippet row(item: (typeof panes)[number])}
        <li>
          <button
            type="button"
            data-pane={item.id}
            aria-current={item.id === pane?.id ? "page" : undefined}
            tabindex={item.id === (found?.[0]?.id ?? pane?.id) ? 0 : -1}
            onclick={() => choose(item.id)}
            ><span class="tile" style:background={`var(--tint-${item.tint})`}><Icon name={item.icon} small /></span
            ><span class="name">{item.title}</span></button
          >
        </li>
      {/snippet}
      {#if found}
        {#if found.length > 0}
          <ul>
            {#each found as item (item.id)}{@render row(item)}{/each}
          </ul>
        {:else}
          <p class="none" role="status">No settings match “{query.trim()}”.</p>
        {/if}
      {:else}
        {#each GROUPS as group (group.id)}
          {@const own = panes.filter((p) => p.group === group.id)}
          {#if own.length > 0}
            <h3 class="group" title={group.name}>{group.name}</h3>
            <ul>
              {#each own as item (item.id)}{@render row(item)}{/each}
            </ul>
          {/if}
        {/each}
      {/if}
    </nav>
  </aside>
    {#if pane}
      <section class="pane" aria-labelledby="settings-pane-title" data-testid={`settings-${pane.id}`}>
        <h2 id="settings-pane-title" class="title">{pane.title}</h2>
        <p class="line">{pane.line}</p>
        {#if pane.id === "trust"}
          <TrustPane />
        {:else if pane.id === "editor"}
          <EditorPane />
        {:else if pane.id === "claude"}
          <h3 class="section">Claude</h3>
          <div class="box">
            <AgentsPane />
            {#each (names.list ?? []).filter((p) => p.id !== "claude") as other (other.id)}
              <ProviderCard id={other.id} />
            {/each}
          </div>
          <h3 class="section">Running tasks</h3>
          <div class="box">
            <AgentLimitField />
            <PlanFirstField />
            <KeepAwakeField />
          </div>
          <!-- Power features stay out of the way until asked for (owner, 2026-11-26). -->
          <details class="extra" bind:open={settings.extra} data-testid="extra-features">
            <summary
              ><Icon name="chevron-right" small /><span>Extra features</span><span class="what"
                >Research agents, several attempts, models, templates, schedules, several repositories</span
              ></summary
            >
            <div class="box">
              <AgentLimitField research />
              <ResourcesField />
              <AttemptsField />
              <ModelField />
              <TemplatesField />
              <SchedulesSwitch />
              <MultiRepoField />
            </div>
          </details>
        {:else if pane.id === "setup"}
          <SetupPane />
        {:else if pane.id === "rules"}
          <PolicyPanel />
        {:else if pane.id === "network"}
          <NetworkPanel />
        {:else if pane.id === "keys"}
          <KeysPane />
        {:else if pane.id === "servers"}
          <ServersPanel />
        {:else if pane.id === "schedules"}
          <SchedulesPane />
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
        {:else if pane.id === "phone"}
          <PhonePane />
        {:else if pane.id === "machines"}
          <MachinesPane />
        {:else if pane.id === "keyboard"}
          <input
            type="search"
            class="filter"
            bind:value={keyFilter}
            placeholder="Filter"
            aria-label="Filter shortcuts"
            autocomplete="off"
            spellcheck="false"
            onkeydown={onFilterKeydown}
            data-testid="keys-filter"
          />
          {#each keyGroups as group, i (group.where)}
            <h4 class="keys-group" id={`settings-keys-${i}`}>{group.where}</h4>
            <table aria-labelledby={`settings-keys-${i}`}>
              <tbody>
                {#each group.rows as shortcut (shortcut.id)}
                  <tr data-command={shortcut.id} data-keys={shortcut.binding}
                    ><td>{shortcut.title}</td><td><kbd>{shortcut.keys}</kbd></td></tr
                  >
                {/each}
              </tbody>
            </table>
          {:else}
            <p class="note">No shortcut matches “{keyFilter.trim()}”.</p>
          {/each}
          <p class="note">Letters follow your Latin layout; on other layouts and for punctuation, the key's place counts.</p>
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

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 90;
    background: var(--scrim);
    animation: fade var(--dur-2) var(--ease-out);
  }
  .window {
    position: fixed;
    inset: 0;
    z-index: 91;
    display: grid;
    grid-template-columns: 220px minmax(0, 1fr);
    width: min(880px, calc(100vw - 48px));
    height: min(640px, calc(100vh - 64px));
    margin: auto;
    overflow: hidden;
    background: var(--s2);
    border-radius: var(--r-lg);
    box-shadow: var(--shadow);
    animation: rise var(--dur-2) var(--ease-out);
  }
  .window:focus {
    outline: none;
  }
  aside {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    min-height: 0;
    padding: var(--sp-3) var(--sp-2) var(--sp-3) var(--sp-3);
    background: var(--s1);
    border-right: 1px solid var(--separator);
  }
  .close {
    display: grid;
    place-items: center;
    width: var(--ctl-sm);
    height: var(--ctl-sm);
    padding: 0;
    color: var(--text-2);
    background: transparent;
    border: 0;
    border-radius: var(--r-pill);
    cursor: default;
  }
  .close:hover {
    color: var(--text);
    background: var(--s3);
  }
  .find {
    position: relative;
    flex: none;
    color: var(--text-3);
  }
  .find :global(.icon) {
    position: absolute;
    top: calc((var(--ctl) - 16px) / 2);
    left: var(--sp-2);
    pointer-events: none;
  }
  .search {
    box-sizing: border-box;
    width: 100%;
    height: var(--ctl);
    padding: 0 var(--sp-2) 0 calc(var(--sp-2) * 2 + 16px);
    font: inherit;
    color: var(--text);
    background: var(--s3);
    border: 0;
    border-radius: var(--r-md);
  }
  nav {
    flex: 1;
    min-height: 0;
    margin-right: calc(-1 * var(--sp-2));
    padding-right: var(--sp-2);
    overflow-y: auto;
  }
  .group {
    margin: var(--sp-3) 0 var(--sp-1) var(--sp-2);
    font: var(--t-caption);
    font-weight: 600;
    color: var(--text-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .none {
    margin: var(--sp-2);
    font: var(--t-small);
    color: var(--text-2);
  }
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  nav button {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    width: 100%;
    height: var(--row);
    padding: 0 var(--sp-1);
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
    color: var(--accent-ink);
    background: var(--accent);
  }
  .tile {
    display: grid;
    flex: none;
    place-items: center;
    width: 20px;
    height: 20px;
    color: var(--tint-ink);
    border-radius: var(--r-sm);
  }
  .tile :global(.icon) {
    width: 14px;
    height: 14px;
    stroke-width: 2;
  }
  .name {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  @keyframes fade {
    from {
      opacity: 0;
    }
  }
  @keyframes rise {
    from {
      opacity: 0;
      transform: scale(0.98);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .scrim,
    .window {
      animation: none;
    }
  }
  .pane {
    min-height: 0;
    padding: var(--sp-5) var(--sp-6) var(--sp-6);
    overflow-y: auto;
  }
  .pane > :global(*) {
    max-width: 600px;
  }
  .section {
    margin: var(--sp-4) 0 var(--sp-2) var(--sp-1);
    font: var(--t-caption);
    font-weight: 600;
    color: var(--text-2);
  }
  /* A group of fields on one inset card, as in System Settings. */
  .box {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    padding: var(--sp-1) var(--sp-4) var(--sp-4);
    background: var(--s1);
    border-radius: var(--r-md);
    box-shadow: 0 0 0 1px var(--separator);
  }
  .extra {
    margin-top: var(--sp-4);
  }
  .extra summary {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    padding: var(--sp-2) var(--sp-1);
    list-style: none;
    border-radius: var(--r-sm);
    cursor: default;
  }
  .extra summary::-webkit-details-marker {
    display: none;
  }
  .extra summary :global(.icon) {
    color: var(--text-2);
    transition: transform var(--dur-1) var(--ease-out);
  }
  .extra[open] summary :global(.icon) {
    transform: rotate(90deg);
  }
  .extra summary:hover {
    background: var(--s3);
  }
  .what {
    overflow: hidden;
    font: var(--t-small);
    color: var(--text-2);
    white-space: nowrap;
    text-overflow: ellipsis;
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
  .filter {
    box-sizing: border-box;
    width: 100%;
    max-width: 480px;
    height: var(--ctl);
    padding: 0 var(--sp-3);
    font: inherit;
    color: var(--text);
    background: var(--s3);
    border: 0;
    border-radius: var(--r-md);
  }
  .keys-group {
    margin: var(--sp-4) 0 var(--sp-1);
    font: var(--t-small);
    font-weight: 600;
    color: var(--text-2);
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
    .window {
      outline: 1px solid CanvasText;
    }
    nav button[aria-current="page"] {
      outline: 2px solid Highlight;
      outline-offset: -2px;
    }
  }
</style>
