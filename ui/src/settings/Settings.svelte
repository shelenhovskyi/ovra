<script lang="ts">
// The Settings view in the main area (Mod+, or the rail's gear): a list of
// panes in two groups, the open project's and Ovra's, and the chosen pane
// beside it. Each pane has a noun for a title, one line on what it does and
// one column of fields. Arrow keys move in the list.
import { untrack } from "svelte";
import { providers } from "../agents/providers.svelte";
import { listCommands } from "../keys/commands";
import { listContextKeys } from "../keys/context-keys";
import { formatShortcut, inBrowserTab, isApple, liveKeys } from "../keys/shortcut";
import type { CoreInfo } from "../lib/protocol";
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
import ViewFrame from "../shell/ViewFrame.svelte";
import { loadScreenReader } from "../terminal/prefs";
import AgentLimitField from "./AgentLimitField.svelte";
import AgentsPane from "./AgentsPane.svelte";
import AttemptsField from "./AttemptsField.svelte";
import EditorPane from "./EditorPane.svelte";
import ModelField from "./ModelField.svelte";
import PlanFirstField from "./PlanFirstField.svelte";
import ProviderCard from "./ProviderCard.svelte";
import { type PaneId, panesFor, settings } from "./panes.svelte";
import ResourcesField from "./ResourcesField.svelte";
import SetupPane from "./SetupPane.svelte";
import TemplatesField from "./TemplatesField.svelte";
import TrustPane from "./TrustPane.svelte";

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

/** Escape in a filter with text clears it; the next one leaves Settings. */
function onFilterKeydown(event: KeyboardEvent): void {
  if (event.code !== "Escape" || !keyFilter) return;
  event.preventDefault();
  keyFilter = "";
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
          <h3 class="group" title={group.name}>{group.name}</h3>
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
        {#if pane.id === "trust"}
          <TrustPane />
        {:else if pane.id === "editor"}
          <EditorPane />
        {:else if pane.id === "claude"}
          <AgentsPane />
          {#each (names.list ?? []).filter((p) => p.id !== "claude") as other (other.id)}
            <ProviderCard id={other.id} />
          {/each}
          <AgentLimitField />
          <AgentLimitField research />
          <ResourcesField />
          <AttemptsField />
          <ModelField />
          <TemplatesField />
          <PlanFirstField />
          <SchedulesSwitch />
          <KeepAwakeField />
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
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
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
    nav button[aria-current="page"] {
      outline: 2px solid Highlight;
      outline-offset: -2px;
    }
  }
</style>
