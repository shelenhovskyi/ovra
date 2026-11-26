<script lang="ts">
// The @ menu and the chips of one message box (docs/specs/task-context.md).
// The box's textarea stays the parent's: it calls `keydown` first from its
// own handler and `refresh` on input, and this component keeps the typed
// `@query`, the popup above the field and the chips under it.
import type { ContextWhat, ProjectId } from "../lib/protocol";
import { CoreError } from "../lib/transport";
import { projectFiles } from "../palette/files";
import { rank } from "../palette/fuzzy";
import { recentFiles } from "../palette/recent-files.svelte";
import { useOvra } from "../shell/context";
import Icon from "../shell/Icon.svelte";
import {
  atQuery,
  contextDrafts,
  contextSuggestions,
  describeItem,
  type Suggestion,
  type Target,
  WARNINGS,
} from "./context.svelte";

let {
  field,
  value = $bindable(),
  project,
  target,
  extra = [],
  id,
}: {
  field: HTMLTextAreaElement | undefined;
  value: string;
  project: ProjectId;
  target: Target;
  /** Offered besides the sources, such as the session's browser page. */
  extra?: Suggestion[];
  /** Prefix of the menu's element ids. */
  id: string;
} = $props();

const { transport } = useOvra();
const drafts = contextDrafts(transport);
const chips = $derived(drafts.of(target));

const MAX_ROWS = 8;

let at = $state<{ start: number; query: string } | null>(null);
/** Where an `@` starts that the person closed with Escape: it stays text. */
let dismissed = $state<number | null>(null);
let active = $state(0);
let error = $state<string | null>(null);
let files = $state<string[] | null>(null);
let place = $state({ left: 0, bottom: 0, width: 280 });

function base(path: string): string {
  return path.slice(path.lastIndexOf("/") + 1);
}

/** Every file and folder as the menu offers them, folders after their files' names. */
const paths = $derived.by((): Suggestion[] => {
  if (!files) return [];
  const folders = new Set<string>();
  for (const file of files) {
    for (let at = file.indexOf("/"); at > 0; at = file.indexOf("/", at + 1)) folders.add(file.slice(0, at));
  }
  return [
    ...files.map((path) => ({ what: { kind: "file", path } as ContextWhat, label: base(path), detail: path })),
    ...[...folders].map((path) => ({
      what: { kind: "folder", path } as ContextWhat,
      label: `${base(path)}/`,
      detail: `${path}/`,
    })),
  ];
});

const options = $derived.by((): Suggestion[] => {
  if (!at) return [];
  const query = at.query;
  const offered = [...contextSuggestions(), ...extra];
  const near = rank(offered, (s) => `${s.label} ${s.detail}`, query, 5).map((r) => r.item);
  const found = query
    ? rank(paths, (s) => s.detail, query, MAX_ROWS).map((r) => r.item)
    : recentFiles(project)
        .slice(0, 4)
        .map((path) => ({ what: { kind: "file", path } as ContextWhat, label: base(path), detail: path }));
  return [...near, ...found].slice(0, MAX_ROWS);
});

const open = $derived(at !== null && options.length > 0);

$effect(() => {
  if (at && files === null) {
    files = [];
    projectFiles(transport, project).then(
      (list) => (files = list),
      () => (files = null),
    );
  }
});

// The field is a combobox while the menu shows.
$effect(() => {
  const box = field;
  if (!box) return;
  box.setAttribute("aria-autocomplete", "list");
  box.setAttribute("aria-expanded", String(open));
  if (open) {
    box.setAttribute("aria-controls", `${id}-menu`);
    box.setAttribute("aria-activedescendant", `${id}-option-${active}`);
  } else {
    box.removeAttribute("aria-controls");
    box.removeAttribute("aria-activedescendant");
  }
});

/** Reads the `@query` at the caret; the parent calls it on input and caret moves. */
export function refresh(): void {
  const box = field;
  const found = box ? atQuery(value, box.selectionStart ?? value.length) : null;
  if (found && found.start === dismissed) {
    at = null;
    return;
  }
  if (dismissed !== null && (!found || found.start !== dismissed)) dismissed = null;
  if (found?.start !== at?.start || found?.query !== at?.query) active = 0;
  at = found;
  if (found && box) {
    const rect = box.getBoundingClientRect();
    place = { left: rect.left, bottom: window.innerHeight - rect.top + 4, width: Math.max(rect.width, 240) };
  }
}

/** Opens the menu as if the person typed `@` at the caret (the @ button). */
export function start(): void {
  const box = field;
  if (!box) return;
  const caret = box.selectionStart ?? value.length;
  const gap = caret > 0 && !/\s/.test(value[caret - 1] ?? "") ? " " : "";
  value = `${value.slice(0, caret)}${gap}@${value.slice(caret)}`;
  box.focus();
  const next = caret + gap.length + 1;
  queueMicrotask(() => {
    box.setSelectionRange(next, next);
    refresh();
  });
}

/** Handles the menu's keys and Backspace in an empty box; true when it took the key. */
export function keydown(event: KeyboardEvent): boolean {
  if (event.isComposing) return false;
  if (open && at) {
    const take = () => {
      event.preventDefault();
      return true;
    };
    switch (event.code) {
      case "ArrowDown":
        active = (active + 1) % options.length;
        return take();
      case "ArrowUp":
        active = (active - 1 + options.length) % options.length;
        return take();
      case "Enter":
      case "NumpadEnter":
      case "Tab": {
        if (event.shiftKey) return false;
        const chosen = options[active];
        if (chosen) choose(chosen);
        return take();
      }
      case "Escape":
        dismissed = at.start;
        at = null;
        event.stopPropagation();
        return take();
    }
  }
  if (event.code === "Backspace" && value === "" && chips.length > 0) {
    const last = chips.at(-1);
    if (last) drafts.remove(target, last.key);
    event.preventDefault();
    return true;
  }
  return false;
}

function choose(option: Suggestion): void {
  const typed = at;
  if (!typed) return;
  value = value.slice(0, typed.start) + value.slice(typed.start + 1 + typed.query.length);
  at = null;
  error = null;
  const box = field;
  queueMicrotask(() => box?.setSelectionRange(typed.start, typed.start));
  drafts.add(project, target, option.what).catch((err: unknown) => {
    error = err instanceof CoreError ? err.error.message : String(err);
  });
}
</script>

{#if open}
  <ul
    class="at-menu"
    id="{id}-menu"
    role="listbox"
    aria-label="Add context"
    data-testid="context-menu"
    style:left="{place.left}px"
    style:bottom="{place.bottom}px"
    style:width="{place.width}px"
  >
    {#each options as option, i (`${option.what.kind} ${option.detail}`)}
      <li
        id="{id}-option-{i}"
        role="option"
        aria-selected={i === active}
        onpointerdown={(event) => {
          event.preventDefault();
          choose(option);
        }}
        onpointermove={() => (active = i)}
      >
        <Icon name={describeItem(option.what).icon} small />
        <span class="name">{option.label}</span>
        <span class="detail">{option.detail}</span>
      </li>
    {/each}
  </ul>
{/if}
{#if chips.length > 0}
  <ul class="ctx-chips" aria-label="Context sent with the message">
    {#each chips as chip (chip.key)}
      {@const waits = chip.warning !== null && !chip.confirmed}
      <li class="ctx-chip" class:waits data-testid="context-chip" title={chip.title}>
        <Icon name={chip.icon} small />
        <span class="label">{chip.label}</span>
        {#if waits && chip.warning}
          <span class="why">{WARNINGS[chip.warning]}</span>
          <button type="button" class="include" onclick={() => drafts.confirm(target, chip.key)}>Include</button>
        {/if}
        <button type="button" class="remove" aria-label="Remove {chip.label}" onclick={() => drafts.remove(target, chip.key)}
          ><Icon name="close" small /></button
        >
      </li>
    {/each}
  </ul>
{/if}
{#if error}
  <p class="ctx-error" role="alert">{error}</p>
{/if}

<style>
  .at-menu {
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
  .at-menu li {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    padding: 5px 8px;
    border-radius: var(--radius);
    cursor: default;
  }
  .at-menu li[aria-selected="true"] {
    color: var(--accent-ink);
    background: var(--accent);
  }
  .at-menu .name {
    flex: none;
    font-weight: 500;
  }
  .at-menu .detail {
    min-width: 0;
    overflow: hidden;
    font-size: 12px;
    white-space: nowrap;
    text-overflow: ellipsis;
    direction: rtl;
    text-align: left;
    opacity: 0.7;
  }
  .ctx-chips {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .ctx-chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    max-width: 100%;
    height: 24px;
    padding: 0 2px 0 6px;
    font-size: 12px;
    color: var(--text-2);
    background: var(--s3);
    border-radius: var(--radius);
  }
  .ctx-chip .label {
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    color: var(--text);
  }
  .ctx-chip.waits {
    box-shadow: inset 0 0 0 1px var(--warn);
  }
  .ctx-chip .why {
    color: var(--warn);
    white-space: nowrap;
  }
  .ctx-chip button {
    height: 20px;
    padding: 0 4px;
    font-size: 12px;
  }
  .ctx-chip .remove {
    display: inline-flex;
    align-items: center;
    opacity: 0.7;
  }
  .ctx-chip .remove:hover {
    opacity: 1;
  }
  .ctx-error {
    margin: 0;
    font-size: 12px;
    color: var(--warn);
  }
</style>
