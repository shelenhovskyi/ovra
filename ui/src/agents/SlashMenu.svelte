<script lang="ts">
// The `/` menu of one message box (docs/specs/task-commands.md): typed `/`
// at the start of the box lists the person's templates and, for Claude, the
// project's commands and skills. Like ContextInput, the textarea stays the
// parent's: it calls `keydown` first from its own handler and `refresh` on
// input.
import type { ProjectId } from "../lib/protocol";
import { openSettings } from "../settings/commands";
import { useOvra } from "../shell/context";
import Icon from "../shell/Icon.svelte";
import { agentPrefs } from "./prefs.svelte";
import { applyRow, type SlashCommand, type SlashRow, slashCommands, slashQuery, slashRows } from "./slash";

let {
  field,
  value = $bindable(),
  project,
  commands: withCommands,
  id,
}: {
  field: HTMLTextAreaElement | undefined;
  value: string;
  project: ProjectId;
  /** Whether the project's commands and skills are offered: the box's agent is Claude. */
  commands: boolean;
  /** Prefix of the menu's element ids. */
  id: string;
} = $props();

const { transport } = useOvra();

/** The typed `/query`; null while the menu is closed. */
let query = $state<string | null>(null);
/** The menu was closed with Escape: `/` stays text until the box is empty again. */
let dismissed = $state(false);
let active = $state(0);
let place = $state({ left: 0, bottom: 0, width: 280 });
/** The project's commands, read once the menu opens; null until then. */
let found = $state<SlashCommand[] | null>(null);
let loading = $state(false);

type Row = SlashRow | { key: "settings"; kind: "settings"; label: string; detail: string };

const SETTINGS_ROW: Row = {
  key: "settings",
  kind: "settings",
  label: "Task templates…",
  detail: "Save texts you use often, in Settings",
};

const rows = $derived.by((): Row[] => {
  if (query === null) return [];
  const listed: Row[] = slashRows(query, agentPrefs.templates, withCommands ? (found ?? []) : []);
  // With no templates yet, the menu says where they come from.
  return agentPrefs.templates.length === 0 && query === "" ? [...listed, SETTINGS_ROW] : listed;
});

const open = $derived(query !== null);

/** Counts the reads, so only the latest one, for the open menu, lands. */
let reads = 0;

// Read the project's commands each time the menu opens, so a new one shows.
$effect(() => {
  if (!open || !withCommands) return;
  const read = ++reads;
  const asked = project;
  found = null;
  loading = true;
  const mine = () => read === reads && open && asked === project;
  void transport.call({ method: "catalog.list", params: { project: asked } }).then(
    ({ data }) => {
      if (!mine()) return;
      found = slashCommands(data.items);
      loading = false;
    },
    () => {
      if (!mine()) return;
      found = [];
      loading = false;
    },
  );
  return () => {
    // Closed or moved to another project: the read on its way is dropped.
    reads++;
    loading = false;
  };
});

// As in the @ menu: the field points at the menu and its active option.
$effect(() => {
  const box = field;
  if (!box || !open) return;
  box.setAttribute("aria-controls", `${id}-menu`);
  if (rows.length > 0) box.setAttribute("aria-activedescendant", `${id}-option-${active}`);
  else box.removeAttribute("aria-activedescendant");
  return () => {
    box.removeAttribute("aria-controls");
    box.removeAttribute("aria-activedescendant");
  };
});

$effect(() => {
  const box = field;
  if (!box) return;
  const leave = () => {
    queueMicrotask(() => {
      if (query !== null) query = null;
    });
  };
  box.addEventListener("blur", leave);
  return () => box.removeEventListener("blur", leave);
});

/** Reads the `/query` at the caret; the parent calls it on input and clicks. */
export function refresh(): void {
  const box = field;
  if (value === "") dismissed = false;
  const typed = box && !dismissed ? slashQuery(value, box.selectionStart ?? value.length) : null;
  if (typed !== query) active = 0;
  query = typed;
  if (typed !== null && box) {
    const rect = box.getBoundingClientRect();
    place = { left: rect.left, bottom: window.innerHeight - rect.top + 4, width: Math.max(rect.width, 240) };
  }
}

/** Handles the menu's keys; true when it took the key. */
export function keydown(event: KeyboardEvent): boolean {
  if (event.isComposing || query === null || event.ctrlKey || event.metaKey || event.altKey) return false;
  const take = () => {
    event.preventDefault();
    return true;
  };
  switch (event.code) {
    case "ArrowDown":
      if (rows.length > 0) active = (active + 1) % rows.length;
      return take();
    case "ArrowUp":
      if (rows.length > 0) active = (active - 1 + rows.length) % rows.length;
      return take();
    case "Enter":
    case "NumpadEnter":
    case "Tab": {
      if (event.shiftKey) return false;
      const chosen = rows[active];
      // With no match, Enter sends the text as typed (Claude may know the command).
      if (!chosen) return loading ? take() : false;
      choose(chosen);
      return take();
    }
    case "Escape":
      dismissed = true;
      query = null;
      event.stopPropagation();
      return take();
  }
  return false;
}

function choose(row: Row): void {
  const typed = query;
  if (typed === null) return;
  query = null;
  if (row.kind === "settings") {
    value = value.slice(1 + typed.length);
    openSettings("claude");
    return;
  }
  const next = applyRow(row, value, typed);
  value = next.value;
  const box = field;
  queueMicrotask(() => {
    box?.focus();
    box?.setSelectionRange(next.caret, next.caret);
  });
}
</script>

{#if open}
  <ul
    class="slash-menu"
    id="{id}-menu"
    role="listbox"
    aria-label="Templates and commands"
    data-testid="slash-menu"
    style:left="{place.left}px"
    style:bottom="{place.bottom}px"
    style:width="{place.width}px"
  >
    {#if rows.length === 0}
      <li class="none" role="option" aria-selected="false" aria-disabled="true" data-testid="slash-menu-empty">
        {loading ? "Looking…" : "No matching template or command"}
      </li>
    {/if}
    {#each rows as row, i (row.key)}
      <li
        id="{id}-option-{i}"
        role="option"
        aria-selected={i === active}
        data-kind={row.kind}
        onpointerdown={(event) => {
          event.preventDefault();
          choose(row);
        }}
        onpointermove={() => (active = i)}
      >
        <Icon name={row.kind === "template" ? "file" : row.kind === "settings" ? "settings" : "terminal"} small />
        <span class="name">{row.label}</span>
        <span class="detail">{row.detail}</span>
      </li>
    {/each}
  </ul>
{/if}

<style>
  .slash-menu {
    position: fixed;
    z-index: 20;
    max-width: 420px;
    max-height: 320px;
    overflow-y: auto;
    margin: 0;
    padding: 6px;
    list-style: none;
    background: var(--s4);
    border-radius: var(--radius-lg);
    box-shadow: var(--card-shadow);
  }
  .slash-menu li {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    padding: 5px 8px;
    border-radius: var(--radius);
    cursor: default;
  }
  .slash-menu li[aria-selected="true"] {
    color: var(--accent-ink);
    background: var(--accent);
  }
  .slash-menu li.none {
    opacity: 0.7;
  }
  .slash-menu .name {
    flex: 0 0 auto;
    max-width: 50%;
    overflow: hidden;
    font-weight: 500;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .slash-menu .detail {
    min-width: 0;
    overflow: hidden;
    font-size: var(--fs-ui-sm);
    white-space: nowrap;
    text-overflow: ellipsis;
    opacity: 0.7;
  }
</style>
