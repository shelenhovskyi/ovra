<script lang="ts">
// The @ menu and the chips of one message box (docs/specs/task-context.md).
// The box's textarea stays the parent's: it calls `keydown` first from its
// own handler and `refresh` on input, and this component keeps the typed
// `@query`, the popup above the field and the chips under it.
import type { ContextWhat, ProjectId } from "../lib/protocol";
import { CoreError, type WindowDrop } from "../lib/transport";
import { projectFiles } from "../palette/files";
import { rank } from "../palette/fuzzy";
import { recentFiles } from "../palette/recent-files.svelte";
import { useOvra } from "../shell/context";
import Icon from "../shell/Icon.svelte";
import type { IconName } from "../shell/icons";
import {
  attachFiles,
  copyDrop,
  dropZone,
  insidePaths,
  isImageName,
  itemFor,
  pastedFiles,
  secretLooking,
  thumbnail,
} from "./attach";
import {
  atQuery,
  contextDrafts,
  contextSuggestions,
  describeItem,
  type Suggestion,
  type Target,
  WARNINGS,
} from "./context.svelte";
import { type GitHubList, githubKey, githubRows, loadGitHub, offersGitHub } from "./github-context";
import { hints } from "./hints.svelte";

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

const { transport, workspace } = useOvra();
const drafts = contextDrafts(transport);
const chips = $derived(drafts.of(target));

const MAX_ROWS = 8;

let at = $state<{ start: number; query: string } | null>(null);
/** The `@query` the person closed with Escape: it stays text until they type more of it. */
let dismissed = $state<{ start: number; query: string } | null>(null);
let active = $state(0);
let error = $state<string | null>(null);
let files = $state<string[] | null>(null);
/** The project's file list is on its way. */
let loadingFiles = $state(false);
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

/** What the box holds already, so the menu leaves it out. */
const added = $derived(new Set(chips.map((chip) => JSON.stringify(chip.what))));

function asFile(path: string): Suggestion {
  return { what: { kind: "file", path }, label: base(path), detail: path };
}

/**
 * A row of the menu: an item to add (`what`), one read when chosen (`load`:
 * a GitHub issue or check's log), or the entry to the GitHub rows (`github`).
 */
type Row = {
  key: string;
  label: string;
  detail: string;
  icon: IconName;
  what?: ContextWhat;
  load?: () => Promise<ContextWhat>;
  github?: true;
};

/** `@#` lists GitHub's rows instead of the project's. */
const inGitHub = $derived(at?.query.startsWith("#") ?? false);
/** What `@#` read; null until asked, `error` when gh could not answer. */
let gh = $state<{ list: GitHubList | null; error: string | null } | null>(null);
/** Counts the reads of GitHub's rows, so only the latest one lands. */
let ghReads = 0;

// Read through the person's gh only once they typed `@#`, again for each menu.
$effect(() => {
  if (inGitHub && gh === null) {
    gh = { list: null, error: null };
    // A menu closed and opened again meanwhile reads anew: an older answer is dropped.
    const read = ++ghReads;
    const mine = () => gh !== null && read === ghReads;
    void loadGitHub(transport, project).then(
      (list) => {
        if (mine()) gh = { list, error: null };
      },
      (err: unknown) => {
        if (mine()) gh = { list: null, error: message(err) };
      },
    );
  }
});
$effect(() => {
  if (at === null) gh = null;
});

const GITHUB_ENTRY: Row = {
  key: "github",
  label: "GitHub issue or failed check",
  detail: "@#",
  icon: "issue",
  github: true,
};

const rows = $derived.by((): Row[] => {
  if (!at) return [];
  if (inGitHub) {
    const list = gh?.list;
    if (!list) return [];
    const taken = new Set(chips.map((chip) => githubKey(chip.what)));
    return githubRows(transport, project, list, at.query.slice(1), MAX_ROWS + 4)
      .filter((row) => !taken.has(row.key))
      .slice(0, MAX_ROWS);
  }
  const plain: Row[] = options.map((s) => ({
    key: `${s.what.kind} ${s.detail}`,
    label: s.label,
    detail: s.detail,
    icon: describeItem(s.what).icon,
    what: s.what,
  }));
  return offersGitHub(at.query) && !loadingFiles ? [...plain.slice(0, MAX_ROWS - 1), GITHUB_ENTRY] : plain;
});

/** The menu is waiting for files or GitHub: a key waits too. */
const loading = $derived(inGitHub ? gh !== null && gh.list === null && gh.error === null : loadingFiles);

const options = $derived.by((): Suggestion[] => {
  if (!at) return [];
  const query = at.query;
  const fresh = (s: Suggestion) => !added.has(JSON.stringify(s.what));
  const offered = [...contextSuggestions(), ...extra].filter(fresh);
  const near = rank(offered, (s) => `${s.label} ${s.detail}`, query, 5).map((r) => r.item);
  let found: Suggestion[];
  if (query) found = rank(paths.filter(fresh), (s) => s.detail, query, MAX_ROWS).map((r) => r.item);
  else {
    // Recent files first, then the project's top level, so an empty @ always offers something.
    const recent = recentFiles(project).slice(0, 4).map(asFile);
    const top = paths.filter((s) => !s.detail.slice(0, -1).includes("/"));
    found = [...recent, ...top].filter(fresh);
  }
  const seen = new Set<string>();
  return [...near, ...found]
    .filter((s) => {
      const key = `${s.what.kind} ${s.detail}`;
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    })
    .slice(0, MAX_ROWS);
});

/** The menu shows while an `@query` is typed, with a "No matches" row when nothing fits. */
const open = $derived(at !== null);
/** A chip removed or the menu's answer, for screen readers. */
let said = $state("");

$effect(() => {
  if (at && files === null) {
    files = [];
    loadingFiles = true;
    void projectFiles(transport, project).then(
      (list) => {
        files = list;
        loadingFiles = false;
      },
      () => {
        files = null;
        loadingFiles = false;
      },
    );
  }
});

// The field stays a multi-line textbox (a textarea cannot be a combobox):
// while the menu shows it points at the menu and its active option, and the
// line below says what was picked. aria-expanded is not allowed on a textbox.
$effect(() => {
  const box = field;
  if (!box) return;
  box.setAttribute("aria-autocomplete", "list");
  if (open) {
    box.setAttribute("aria-controls", `${id}-menu`);
    if (rows.length > 0) box.setAttribute("aria-activedescendant", `${id}-option-${active}`);
    else box.removeAttribute("aria-activedescendant");
  } else {
    box.removeAttribute("aria-controls");
    box.removeAttribute("aria-activedescendant");
  }
});

// The menu follows the caret (arrow keys, clicks) and closes when the field loses focus.
$effect(() => {
  const box = field;
  if (!box) return;
  const follow = (event: KeyboardEvent) => {
    if (["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.code)) refresh();
  };
  // Removing the focused field fires blur while Svelte updates the DOM, where
  // state must not change: close the menu just after.
  const leave = () => {
    queueMicrotask(() => {
      if (at !== null) at = null;
    });
  };
  box.addEventListener("keyup", follow);
  box.addEventListener("blur", leave);
  return () => {
    box.removeEventListener("keyup", follow);
    box.removeEventListener("blur", leave);
  };
});

/** Files are dragged over the box. */
let fileOver = $state(false);

// Files dropped on the box or pasted into the field become chips (src/agents/attach.ts).
$effect(() => {
  const input = field;
  const box = input?.closest("form") ?? input;
  if (!input || !box) return;
  const off = dropZone(box, transport, {
    over: (on) => {
      fileOver = on;
      box.toggleAttribute("data-file-over", on);
    },
    window: (drop) => void addDrop(drop),
    rows: (paths) =>
      void addPaths(async () => {
        rootOf();
        return paths;
      }),
    files: (files) => void addFiles(files),
  });
  const paste = (event: ClipboardEvent) => {
    const files = pastedFiles(event.clipboardData);
    if (files.length === 0) return;
    event.preventDefault();
    void addFiles(files);
  };
  input.addEventListener("paste", paste);
  return () => {
    off();
    box.removeAttribute("data-file-over");
    input.removeEventListener("paste", paste);
  };
});

function rootOf(): string {
  const open = workspace.project;
  if (open?.id !== project) throw new Error("Open the task's project to add files to it.");
  return open.root;
}

function message(err: unknown): string {
  return err instanceof CoreError ? err.error.message : err instanceof Error ? err.message : String(err);
}

/**
 * Files from outside the project whose names look like secrets, waiting for
 * Copy anyway. `others` counts the files of the same drop that wait with
 * them: a drop is copied as one.
 */
let pendingCopy = $state<{ names: string[]; others: number; copy: () => Promise<string[]> } | null>(null);
/** A file was just copied into the attachments folder for the first time: say why it is there. */
let copiedNote = $state(false);

function addDrop(drop: WindowDrop): Promise<void> {
  let inside: string[] | null;
  try {
    inside = insidePaths(rootOf(), drop);
  } catch (err) {
    error = message(err);
    return Promise.resolve();
  }
  if (inside) {
    const paths = inside;
    return addPaths(async () => paths);
  }
  return copyChecked(drop.paths, () => copyDrop(transport, project, drop));
}

/** Pasted files are copied one by one: the safe ones go in at once, only the rest wait. */
async function addFiles(files: File[]): Promise<void> {
  const name = (file: File) => file.name || "pasted";
  const risky = await riskyNames(files.map(name));
  if (risky === null) return;
  const safe = files.filter((file) => !risky.includes(name(file)));
  const held = files.filter((file) => risky.includes(name(file)));
  if (safe.length > 0) await addCopies(() => attachFiles(transport, project, safe));
  if (held.length > 0)
    pendingCopy = { names: held.map(name), others: 0, copy: () => attachFiles(transport, project, held) };
}

/** Copies a drop into the project, after Copy anyway when a name looks like a secret: a copy is there for every agent. */
async function copyChecked(names: string[], copy: () => Promise<string[]>): Promise<void> {
  const risky = await riskyNames(names);
  if (risky === null) return;
  if (risky.length > 0) {
    pendingCopy = { names: risky, others: names.length - risky.length, copy };
    return;
  }
  await addCopies(copy);
}

/** The names that look like secrets; null when the check failed (and says so). */
async function riskyNames(names: string[]): Promise<string[] | null> {
  error = null;
  pendingCopy = null;
  try {
    return await secretLooking(transport, project, names);
  } catch (err) {
    error = message(err);
    return null;
  }
}

/** Adds copies as chips; the first time, says why the attachments folder is in the project. */
async function addCopies(copy: () => Promise<string[]>): Promise<void> {
  await addPaths(async () => {
    const paths = await copy();
    if (paths.length > 0 && !hints.has("attached")) {
      copiedNote = true;
      hints.mark("attached");
    }
    return paths;
  });
}

function copyAnyway(): void {
  const waiting = pendingCopy;
  pendingCopy = null;
  if (waiting) void addCopies(waiting.copy);
}

/** "Copy .env anyway" for one name; "Copy them anyway" for several or a whole drop. */
function askNames(ask: { names: string[]; others: number }): string {
  return ask.names.length === 1 && ask.others === 0 ? (ask.names[0] ?? "") : "them";
}

/** Adds the project paths `get` resolves to as chips; says what failed. */
async function addPaths(get: () => Promise<string[]>): Promise<void> {
  error = null;
  try {
    const paths = await get();
    for (const path of paths) await drafts.add(project, target, await itemFor(transport, project, path));
    if (paths.length > 0) said = `Added ${paths.map((p) => p.slice(p.lastIndexOf("/") + 1)).join(", ")}`;
  } catch (err) {
    error = message(err);
  }
  field?.focus();
}

/** Reads the `@query` at the caret; the parent calls it on input and caret moves. */
export function refresh(): void {
  const box = field;
  const found = box ? atQuery(value, box.selectionStart ?? value.length) : null;
  if (found && found.start === dismissed?.start && found.query === dismissed.query) {
    at = null;
    return;
  }
  // Moving away from the closed `@query`, or typing more of it, opens the menu again.
  if (dismissed !== null) dismissed = null;
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
  if (open && at && !event.ctrlKey && !event.metaKey && !event.altKey) {
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
        // While files load, a key waits for them; with no match, Enter sends
        // the message as typed and Tab moves on.
        if (!chosen) return loading ? take() : false;
        choose(chosen);
        return take();
      }
      case "Escape":
        dismissed = { start: at.start, query: at.query };
        at = null;
        event.stopPropagation();
        return take();
    }
  }
  if (event.code === "Backspace" && value === "" && chips.length > 0) {
    // A held Backspace stops at the chips: one press takes one out.
    event.preventDefault();
    if (event.repeat) return true;
    const last = chips.at(-1);
    if (last) removeChip(last.key, last.label);
    return true;
  }
  return false;
}

function removeChip(key: string, label: string): void {
  drafts.remove(target, key);
  said = `Removed ${label}`;
  field?.focus();
}

function sendAnyway(key: string): void {
  drafts.confirm(target, key);
  field?.focus();
}

/** Adds the chosen item as a chip; its name takes the typed `@query`'s place, so the sentence still reads. */
function choose(option: Row): void {
  const typed = at;
  if (!typed) return;
  const rest = value.slice(typed.start + 1 + typed.query.length);
  if (option.github) {
    // The entry types `@#` for the person: the menu stays open on GitHub's rows.
    value = `${value.slice(0, typed.start)}@#${rest}`;
    const caret = typed.start + 2;
    queueMicrotask(() => {
      field?.setSelectionRange(caret, caret);
      refresh();
    });
    return;
  }
  const name = /^\s/.test(rest) ? option.label : `${option.label} `;
  value = value.slice(0, typed.start) + name + rest;
  at = null;
  error = null;
  const box = field;
  const caret = typed.start + name.length;
  queueMicrotask(() => box?.setSelectionRange(caret, caret));
  const picked = option.what;
  const load = option.load;
  if (load) said = `Reading ${option.label}…`;
  // A picked image goes as one when it is small enough, like a dropped one.
  const what =
    picked?.kind === "file" && isImageName(picked.path)
      ? itemFor(transport, project, picked.path)
      : picked && Promise.resolve(picked);
  const added = what
    ? what.then((item) => drafts.add(project, target, item))
    : load
      ? load().then((read) => drafts.add(project, target, read))
      : null;
  added?.then(
    () => (said = `Added ${option.label}`),
    (err: unknown) => {
      error = message(err);
      // An issue or log that could not be read leaves no chip: the typed
      // `@#query` comes back, so "Fix #12" is not sent without the issue.
      if (load && value.slice(typed.start, caret) === name) {
        value = `${value.slice(0, typed.start)}@${typed.query}${value.slice(caret)}`;
      }
    },
  );
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
    {#if rows.length === 0}
      <li class="none" role="option" aria-selected="false" aria-disabled="true" data-testid="context-menu-empty">
        {#if inGitHub}
          {loading ? "Reading GitHub…" : (gh?.error ?? "No open issues or failed checks")}
        {:else}
          {loading ? "Looking…" : "No matches"}
        {/if}
      </li>
    {/if}
    {#each rows as option, i (option.key)}
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
        <Icon name={option.icon} small />
        <span class="name">{option.label}</span>
        <span class="detail">{option.detail}</span>
      </li>
    {/each}
  </ul>
{/if}
{#if fileOver}
  <p class="ctx-drop" data-testid="context-drop">Drop to add to the message</p>
{/if}
{#if pendingCopy}
  {@const them = askNames(pendingCopy)}
  <p class="ctx-ask" role="alert" data-testid="context-copy-ask">
    <span
      >{pendingCopy.names.join(", ")} may hold secrets. A copy in the project can be read by every agent.{#if pendingCopy.others > 0}
        The drop goes in as one, so skipping leaves out its {pendingCopy.others} other {pendingCopy.others === 1
          ? "file"
          : "files"} too.{/if}</span
    >
    <button type="button" class="include" onclick={copyAnyway}>Copy {them} anyway</button>
    <button type="button" onclick={() => (pendingCopy = null)}>Skip {them}</button>
  </p>
{/if}
{#if chips.length > 0}
  <ul class="ctx-chips" aria-label="Context sent with the message">
    {#each chips as chip (chip.key)}
      {@const waits = chip.warning !== null && !chip.confirmed}
      {@const included = chip.warning !== null && chip.confirmed}
      {@const thumb = chip.what.kind === "image" ? thumbnail(project, chip.what.path) : undefined}
      <li
        class="ctx-chip"
        class:waits
        class:included
        class:image={thumb !== undefined}
        data-testid="context-chip"
        title={included ? `${chip.title}\nSent anyway` : chip.title}
      >
        {#if thumb}
          <img class="thumb" src={thumb} alt="" data-testid="context-thumb" />
        {:else}
          <Icon name={chip.icon} small />
        {/if}
        <span class="label">{chip.label}</span>
        {#if waits && chip.warning}
          <span class="why">{WARNINGS[chip.warning]}</span>
          <button
            type="button"
            class="include"
            aria-label="Send {chip.label} anyway"
            onclick={() => sendAnyway(chip.key)}>Send anyway</button
          >
        {/if}
        <button type="button" class="remove" aria-label="Remove {chip.label}" onclick={() => removeChip(chip.key, chip.label)}
          ><Icon name="close" small /></button
        >
      </li>
    {/each}
  </ul>
{/if}
{#if copiedNote && chips.some((chip) => chip.title.startsWith(".ovra-attachments/"))}
  <p class="ctx-note" data-testid="context-copied-note">
    Copied into the project's .ovra-attachments folder so the agent can read it. Git ignores the folder.
  </p>
{/if}
{#if error}
  <p class="ctx-error" role="alert">{error}</p>
{/if}
<span class="ctx-said" aria-live="polite">{said}</span>

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
  .at-menu li.none {
    opacity: 0.7;
  }
  .at-menu .name {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    font-weight: 500;
    white-space: nowrap;
    text-overflow: ellipsis;
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
  .ctx-chip.image {
    height: 40px;
    padding-left: 3px;
  }
  .ctx-chip .thumb {
    width: 34px;
    height: 34px;
    object-fit: cover;
    border-radius: calc(var(--radius) - 1px);
    background: var(--s4);
  }
  .ctx-chip .label {
    min-width: 0;
    max-width: 24ch;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    color: var(--text);
  }
  .ctx-chip.waits {
    box-shadow: inset 0 0 0 1px var(--warn);
  }
  .ctx-chip.included {
    box-shadow: inset 0 0 0 1px var(--separator);
  }
  .ctx-chip .why {
    color: var(--warn);
    white-space: nowrap;
  }
  .ctx-chip button {
    color: inherit;
    background: transparent;
    border: 0;
    border-radius: var(--radius);
    height: 20px;
    padding: 0 4px;
    font-size: 12px;
  }
  .ctx-chip button:hover {
    background: var(--s4);
  }
  .ctx-chip .include {
    font-weight: 500;
    color: var(--warn);
  }
  .ctx-chip .remove {
    display: inline-flex;
    align-items: center;
    opacity: 0.7;
  }
  .ctx-chip .remove:hover {
    opacity: 1;
  }
  .ctx-said {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
  }
  .ctx-ask {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    margin: 0;
    font-size: 12px;
    color: var(--warn);
  }
  .ctx-ask button {
    height: 22px;
    padding: 0 6px;
    font-size: 12px;
    color: var(--text);
    background: var(--s3);
    border: 0;
    border-radius: var(--radius);
  }
  .ctx-ask .include {
    color: var(--warn);
  }
  .ctx-drop {
    margin: 0;
    font-size: 12px;
    color: var(--accent);
  }
  :global(form[data-file-over]) {
    box-shadow: inset 0 0 0 2px var(--accent);
  }
  .ctx-note {
    margin: 0;
    font-size: 12px;
    color: var(--text-2);
  }
  .ctx-error {
    margin: 0;
    font-size: 12px;
    color: var(--warn);
  }
</style>
