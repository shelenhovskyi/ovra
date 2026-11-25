<script lang="ts">
// Project-wide content search: a query with case, word and regex toggles and
// include/exclude globs; results grouped by file. Choosing a result opens the
// file in the editor with the match selected. With Replace shown, matches are
// replaced all at once (after asking), per file or per line (replace.ts).
import { tick } from "svelte";
import Code from "../editor/highlight/Code.svelte";
import { highlightLines, type Segment, sliceSegments } from "../editor/highlight/static";
import { languageFor } from "../editor/languages";
import type { FileMatches, LineMatch, ProjectId } from "../lib/protocol";
import type { Transport } from "../lib/transport";
import EmptyState from "../shell/EmptyState.svelte";
import Icon from "../shell/Icon.svelte";
import type { TextSpot } from "../shell/workspace.svelte";
import { SearchModel } from "./model.svelte";
import { describe, type OpenBuffer, replaceInFiles } from "./replace";

let {
  transport,
  project,
  onopen,
  buffer = () => undefined,
  onleave = () => {},
}: {
  transport: Transport;
  project: ProjectId;
  onopen: (path: string, at: TextSpot) => void;
  /** Escape in the search or replace text: back to the open file. */
  onleave?: () => void;
  /** The open editor buffer of `path`, if any; replacing there leaves the change unsaved. */
  buffer?: (path: string) => OpenBuffer | undefined;
} = $props();

/** Lines rendered at first and added by "Show more lines". */
const PAGE = 500;

// svelte-ignore state_referenced_locally
const model = new SearchModel(transport, project);
$effect(() => () => model.dispose());

let input = $state<HTMLInputElement | null>(null);
let results = $state<HTMLElement | null>(null);
let shownLines = $state(PAGE);
let showFilters = $state(false);
let showReplace = $state(false);
let replacement = $state("");
let replaceInput = $state<HTMLInputElement | null>(null);
/** "Replace all" waits for this answer. */
let confirming = $state(false);
let replacing = $state(false);
/** What the last replace did. */
let note = $state("");
/** Results replaced in an open buffer: the disk, which the search reads, still has them. */
let hidden = $state.raw(new Set<string>());

/** Focuses the search text with it selected. */
export function focus(): void {
  input?.focus();
  input?.select();
}

/** Searches only files matching `include`, filters shown, and focuses the search text. */
export function limitTo(include: string): void {
  model.include = include;
  showFilters = true;
  changed();
  focus();
}

/** Shows the Replace field and focuses it, or the search text while that is empty. */
export async function focusReplace(): Promise<void> {
  showReplace = true;
  await tick();
  const field = model.pattern ? replaceInput : input;
  field?.focus();
  field?.select();
}

/** The results, without those already replaced in open buffers. */
const files = $derived.by(() => {
  if (hidden.size === 0) return model.files;
  return model.files
    .map((file) => ({ path: file.path, lines: file.lines.filter((match) => !hidden.has(resultKey(file.path, match))) }))
    .filter((file) => file.lines.length > 0);
});
const lineCount = $derived(files.reduce((sum, file) => sum + file.lines.length, 0));
const matchCount = $derived(files.reduce((sum, file) => sum + file.lines.reduce((n, l) => n + l.ranges.length, 0), 0));
const canReplace = $derived(model.status.kind === "done" && matchCount > 0 && !replacing);

/** Files to render, cut so that at most `shownLines` lines are in the DOM. */
const visible = $derived.by(() => {
  const out: FileMatches[] = [];
  let left = shownLines;
  for (const file of files) {
    if (left <= 0) break;
    out.push(file.lines.length <= left ? file : { path: file.path, lines: file.lines.slice(0, left) });
    left -= file.lines.length;
  }
  return out;
});

const plural = (n: number, one: string, many: string) => `${n.toLocaleString("en-US")} ${n === 1 ? one : many}`;

const summary = $derived.by(() => {
  const status = model.status;
  const found = `${plural(lineCount, "matching line", "matching lines")} in ${plural(files.length, "file", "files")}`;
  switch (status.kind) {
    case "idle":
      return "";
    case "error":
      return status.message;
    case "searching":
      return files.length ? `Searching… ${found} so far` : "Searching…";
    case "done":
      if (status.stopped) return files.length ? `Stopped: ${found} so far.` : "Stopped.";
      // After a replace, the note says what happened; no matches left is the point.
      if (!files.length) return note ? "" : "No results.";
      return status.truncated ? `${found}. Limit reached: narrow the search.` : `${found}.`;
  }
  return "";
});

function changed(): void {
  shownLines = PAGE;
  hidden = new Set();
  note = "";
  confirming = false;
  model.schedule();
}

/** Replaces the matches of `targets` (only line `only` with one file), then searches again. */
async function replace(targets: FileMatches[], only?: number): Promise<void> {
  if (replacing) return;
  replacing = true;
  confirming = false;
  const query = {
    pattern: model.pattern,
    regex: model.regex,
    caseSensitive: model.caseSensitive,
    wholeWord: model.wholeWord,
    replacement,
  };
  try {
    const out = await replaceInFiles({ transport, project, buffer }, targets, query, only);
    const next = new Set(hidden);
    for (const file of targets) {
      if (!out.files.includes(file.path) || !buffer(file.path)) continue;
      for (const match of file.lines)
        if (only === undefined || match.line === only) next.add(resultKey(file.path, match));
    }
    hidden = next;
    // Open files change in their tabs only: the disk keeps the old text until they are saved.
    const unsaved = out.files.filter((path) => buffer(path)).length;
    const who = unsaved === 1 ? "1 open file has" : `${unsaved} open files have`;
    note = unsaved > 0 ? `${describe(out)} ${who} unsaved changes.` : describe(out);
  } finally {
    replacing = false;
  }
  await model.run();
}

function onReplaceKeydown(event: KeyboardEvent): void {
  if (event.code === "Escape") {
    event.preventDefault();
    onleave();
    return;
  }
  if (event.code !== "Enter" && event.code !== "NumpadEnter") return;
  event.preventDefault();
  if (canReplace) confirming = true;
}

function cancelConfirm(): void {
  confirming = false;
  replaceInput?.focus();
}

function onConfirmKeydown(event: KeyboardEvent): void {
  if (event.code !== "Escape") return;
  event.preventDefault();
  event.stopPropagation();
  cancelConfirm();
}

let confirmButton = $state<HTMLButtonElement | null>(null);
$effect(() => confirmButton?.focus());

/** Whether any option narrows the search, so "no results" can offer to clear them. */
const narrowed = $derived(model.caseSensitive || model.wholeWord || model.regex || !!model.include || !!model.exclude);
const nothingFound = $derived(model.status.kind === "done" && !model.status.stopped && files.length === 0);

function clearOptions(): void {
  model.caseSensitive = model.wholeWord = model.regex = false;
  model.include = model.exclude = "";
  changed();
}

function toggle(option: "caseSensitive" | "wholeWord" | "regex"): void {
  model[option] = !model[option];
  changed();
}

/** The result that is the list's one Tab stop: the last one focused while it is shown, else the first. */
let current = $state<string | null>(null);
const resultKey = (path: string, match: LineMatch) => `${path}\n${match.line}`;
const stop = $derived.by(() => {
  const keys = visible.flatMap((file) => file.lines.map((match) => resultKey(file.path, match)));
  return current !== null && keys.includes(current) ? current : (keys[0] ?? null);
});

function onQueryKeydown(event: KeyboardEvent): void {
  if (event.code === "ArrowDown") {
    const first = results?.querySelector<HTMLButtonElement>("button[data-result][tabindex='0']");
    if (!first) return;
    event.preventDefault();
    first.focus();
  } else if (event.key === "Enter") {
    event.preventDefault();
    shownLines = PAGE;
    hidden = new Set();
    void model.run();
  } else if (event.key === "Escape") {
    event.preventDefault();
    if (model.status.kind === "searching") model.stop();
    else onleave();
  }
}

/** Arrow keys move between results; Home and End go to the first and last. */
function onResultsKeydown(event: KeyboardEvent): void {
  if (event.code === "Escape") {
    event.preventDefault();
    input?.focus();
    return;
  }
  if (!results || !["ArrowDown", "ArrowUp", "Home", "End"].includes(event.code)) return;
  const buttons = [...results.querySelectorAll<HTMLButtonElement>("button[data-result]")];
  if (!buttons.length) return;
  const at = buttons.indexOf(document.activeElement as HTMLButtonElement);
  const next =
    event.code === "Home"
      ? 0
      : event.code === "End"
        ? buttons.length - 1
        : Math.min(Math.max(at + (event.code === "ArrowDown" ? 1 : -1), 0), buttons.length - 1);
  event.preventDefault();
  buttons[next]?.focus();
}

/** Syntax colour of result lines by path, line and text; each file's lines are highlighted as one text. */
let painted = $state.raw(new Map<string, Segment[]>());
const paintKey = (path: string, match: LineMatch) => `${path}\n${match.line}\n${match.text}`;
const asked = new Set<string>();
$effect(() => {
  for (const file of visible) {
    const keys = file.lines.map((match) => paintKey(file.path, match));
    const all = keys.join("\n");
    if (asked.has(all)) continue;
    asked.add(all);
    void highlightLines(
      languageFor(file.path),
      file.lines.map((match) => match.text),
    ).then((lines) => {
      if (!lines) return;
      const next = new Map(painted);
      keys.forEach((key, i) => {
        const segments = lines[i];
        if (segments) next.set(key, segments);
      });
      painted = next;
    });
  }
});

/** Characters kept before the first match when a line is shortened for the narrow sidebar. */
const LEAD = 12;

/**
 * The line's text cut into plain and matched parts. Indentation is dropped, and a
 * line whose first match starts far in begins shortly before it, after "…".
 */
function parts(match: LineMatch): { text: string; hit: boolean; from: number }[] {
  const first = match.ranges[0]?.[0] ?? 0;
  const indent = match.text.length - match.text.trimStart().length;
  const start = Math.min(first, first - indent > 2 * LEAD ? first - LEAD : indent);
  const out: { text: string; hit: boolean; from: number }[] = [];
  if (start > indent || match.column > 0) out.push({ text: "…", hit: false, from: -1 });
  let at = start;
  for (const [from, to] of match.ranges) {
    if (to <= at) continue;
    if (from > at) out.push({ text: match.text.slice(at, from), hit: false, from: at });
    out.push({ text: match.text.slice(Math.max(from, at), to), hit: true, from: Math.max(from, at) });
    at = to;
  }
  if (at < match.text.length) out.push({ text: match.text.slice(at), hit: false, from: at });
  return out;
}

/** The result last opened: the list's selection. */
let opened = $state<string | null>(null);

function open(path: string, match: LineMatch): void {
  opened = resultKey(path, match);
  const [from, to] = match.ranges[0] ?? [0, 0];
  onopen(path, { line: match.line, from: from + match.column, to: to + match.column });
}

const name = (path: string) => path.slice(path.lastIndexOf("/") + 1);
const folder = (path: string) => path.slice(0, Math.max(path.lastIndexOf("/"), 0));
</script>

<section class="search" aria-label="Search in files" data-testid="search">
  <div class="query">
    <input
      type="search"
      placeholder="Search in files"
      aria-label="Search text"
      aria-describedby={visible.length > 0 ? "search-keys" : undefined}
      spellcheck="false"
      autocomplete="off"
      bind:this={input}
      bind:value={model.pattern}
      oninput={changed}
      onkeydown={onQueryKeydown}
    />
    <div class="toggles">
      <button
        type="button"
        title="Replace"
        aria-label="Replace"
        aria-expanded={showReplace}
        onclick={() => (showReplace ? (showReplace = false) : void focusReplace())}
        ><Icon name={showReplace ? "chevron" : "chevron-right"} small /></button
      >
      <button
        type="button"
        title="Match case"
        aria-label="Match case"
        aria-pressed={model.caseSensitive}
        onclick={() => toggle("caseSensitive")}>Aa</button
      >
      <button
        type="button"
        title="Match whole word"
        aria-label="Match whole word"
        aria-pressed={model.wholeWord}
        onclick={() => toggle("wholeWord")}><u>ab</u></button
      >
      <button
        type="button"
        title="Use regular expression"
        aria-label="Use regular expression"
        aria-pressed={model.regex}
        onclick={() => toggle("regex")}>.*</button
      >
      <button
        type="button"
        title="Files to include or exclude"
        aria-label="Files to include or exclude"
        aria-expanded={showFilters}
        onclick={() => (showFilters = !showFilters)}><Icon name="filter" small /></button
      >
    </div>
  </div>
  {#if showReplace}
    <div class="replace">
      <input
        type="text"
        placeholder="Replace"
        aria-label="Replace with"
        spellcheck="false"
        autocomplete="off"
        bind:this={replaceInput}
        bind:value={replacement}
        onkeydown={onReplaceKeydown}
      />
      <button type="button" class="quiet" disabled={!canReplace} onclick={() => (confirming = true)}>Replace all</button>
    </div>
    {#if confirming}
      <div class="confirm" role="alertdialog" aria-label="Replace all" tabindex="-1" onkeydown={onConfirmKeydown}>
        <p>Replace {plural(matchCount, "match", "matches")} in {plural(files.length, "file", "files")}?</p>
        <div class="buttons">
          <button type="button" class="danger" bind:this={confirmButton} onclick={() => void replace(files)}
            >Replace</button
          >
          <button type="button" class="quiet" onclick={cancelConfirm}>Cancel</button>
        </div>
      </div>
    {/if}
    {#if note}<p class="note" role="status" data-testid="replace-note">{note}</p>{/if}
  {/if}
  {#if showFilters}
    <label>
      Files to include
      <input type="text" placeholder="*.ts, src" spellcheck="false" bind:value={model.include} oninput={changed} />
    </label>
    <label>
      Files to exclude
      <input type="text" placeholder="*.test.ts, docs" spellcheck="false" bind:value={model.exclude} oninput={changed} />
    </label>
  {/if}
  <p class="summary" class:error={model.status.kind === "error"} role="status" data-testid="search-summary">{summary}</p>
  {#if model.status.kind === "idle"}
    <EmptyState headline="Search in files" line="Searches every file except ignored ones." />
  {:else if nothingFound && !note}
    <EmptyState
      headline={`No results for "${model.pattern}"`}
      line={narrowed ? "Check the spelling, or clear the options." : "Check the spelling."}
      actions={narrowed ? [{ label: "Clear options", run: clearOptions }] : []}
    />
  {/if}
  {#if visible.length > 0}
    <p class="keys" id="search-keys">Down arrow: results. Escape: back.</p>
  {/if}
  <!-- Arrow keys move between the result buttons inside. -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <ul class="results" aria-label="Search results" bind:this={results} onkeydown={onResultsKeydown}>
    {#each visible as file (file.path)}
      <li class="file">
        <div class="file-name" title={file.path}>
          <span class="name">{name(file.path)}</span>
          <span class="folder">{folder(file.path)}</span>
          <span class="count" aria-label={plural(file.lines.length, "matching line", "matching lines")}
            >{file.lines.length}</span
          >
          {#if showReplace}
            <button
              type="button"
              class="row-action"
              tabindex="-1"
              title="Replace in file"
              aria-label={`Replace in ${file.path}`}
              disabled={!canReplace}
              onclick={() => void replace(files.filter((f) => f.path === file.path))}>Replace</button
            >
          {/if}
        </div>
        <ul>
          {#each file.lines as match (match.line)}
            {@const line = painted.get(paintKey(file.path, match))}
            <li>
              <button
                type="button"
                class="line"
                data-result
                tabindex={resultKey(file.path, match) === stop ? 0 : -1}
                aria-current={resultKey(file.path, match) === opened ? "true" : undefined}
                onfocus={() => (current = resultKey(file.path, match))}
                aria-label={`${file.path} line ${match.line}: ${match.text.trim()}`}
                onclick={() => open(file.path, match)}
              >
                <span class="num" aria-hidden="true">{match.line}</span>
                <span class="text" aria-hidden="true"
                  >{#each parts(match) as part, i (i)}{@const code = line && part.from >= 0
                      ? sliceSegments(line, part.from, part.from + part.text.length)
                      : null}{#if part.hit}<mark><Code text={part.text} segments={code} /></mark>{:else}<Code
                        text={part.text}
                        segments={code}
                      />{/if}{/each}</span
                >
              </button>
              {#if showReplace}
                <button
                  type="button"
                  class="row-action"
                  tabindex="-1"
                  title="Replace"
                  aria-label={`Replace on ${file.path} line ${match.line}`}
                  disabled={!canReplace}
                  onclick={() => void replace([file], match.line)}>Replace</button
                >
              {/if}
            </li>
          {/each}
        </ul>
      </li>
    {/each}
  </ul>
  {#if lineCount > shownLines}
    <button type="button" class="more" onclick={() => (shownLines += PAGE)}>
      Show {plural(lineCount - shownLines, "more line", "more lines")}
    </button>
  {/if}
</section>

<style>
  .search {
    display: flex;
    flex-direction: column;
    gap: 8px;
    height: 100%;
    min-height: 0;
    padding: 12px 10px 8px;
    box-sizing: border-box;
    background: var(--s1);
  }
  .query {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  input {
    width: 100%;
    box-sizing: border-box;
    padding: 6px 10px;
    font: inherit;
    color: inherit;
    background: var(--s3);
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
  }
  .toggles {
    display: flex;
    gap: 2px;
  }
  /* Secondary chrome: muted until hovered or focused. */
  .toggles button {
    min-width: 28px;
    padding: 2px 6px;
    font: 13px var(--mono);
    color: var(--text-2);
    background: transparent;
    border: 0;
    border-radius: var(--radius-sm);
    cursor: pointer;
  }
  .toggles button:hover {
    color: var(--text);
    background: var(--s3);
  }
  .toggles button[aria-pressed="true"],
  .toggles button[aria-expanded="true"] {
    color: var(--accent);
    background: var(--accent-soft);
  }
  .replace {
    display: flex;
    gap: 4px;
  }
  .replace input {
    flex: 1;
    min-width: 0;
  }
  .quiet,
  .row-action {
    flex: none;
    min-height: 26px;
    padding: 0 8px;
    font: inherit;
    font-size: 13px;
    color: var(--text-2);
    background: transparent;
    border: 0;
    border-radius: var(--radius-sm);
    cursor: default;
  }
  .quiet:hover:not(:disabled),
  .row-action:hover:not(:disabled) {
    color: var(--text);
    background: var(--s3);
  }
  .quiet:disabled,
  .row-action:disabled {
    color: var(--text-2);
    background: var(--s3);
  }
  .confirm {
    padding: 10px 12px;
    border-radius: var(--radius-sm);
    background: var(--s4);
    box-shadow: var(--card-shadow);
  }
  .confirm p {
    margin: 0 0 8px;
  }
  .buttons {
    display: flex;
    gap: 6px;
  }
  .danger {
    min-height: 26px;
    padding: 0 10px;
    font: inherit;
    font-weight: 500;
    color: var(--danger);
    background: var(--danger-soft);
    border: 0;
    border-radius: var(--radius-sm);
  }
  .confirm button:focus-visible,
  .row-action:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  .note {
    margin: 0;
    font-size: 13px;
    color: var(--text-2);
  }
  /* Per-file and per-line replace: shown on hover or focus of the row. */
  .row-action {
    display: none;
    min-height: 22px;
    background: var(--s3);
  }
  .file-name:hover .row-action,
  .file-name:focus-within .row-action,
  .results li:hover > .row-action,
  .results li:focus-within > .row-action {
    display: block;
  }
  .results li {
    position: relative;
  }
  .results li > .row-action {
    position: absolute;
    top: 3px;
    right: 4px;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 13px;
    color: var(--text-2);
  }
  .summary,
  /* Read out with the field; the keys are the usual ones. */
  .keys {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
  .summary:empty {
    display: none;
  }
  .summary.error {
    color: var(--danger);
  }
  .results {
    flex: 1;
    min-height: 0;
    overflow: auto;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .results ul {
    margin: 0 0 6px;
    padding: 0;
    list-style: none;
  }
  .file-name {
    display: flex;
    gap: 8px;
    align-items: center;
    min-height: var(--ctl);
    padding: 0 4px;
    white-space: nowrap;
    overflow: hidden;
  }
  .name {
    color: var(--text);
    font-weight: 500;
  }
  .folder {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: 13px;
    color: var(--text-2);
  }
  .count {
    padding: 0 7px;
    font-size: 12px;
    border-radius: var(--radius-lg);
    color: var(--text-2);
    background: var(--s3);
  }
  /* A 28 px row; the opened result is the selection pill. */
  .line {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    min-height: var(--ctl);
    padding: 0 4px 0 8px;
    font: 13px var(--mono);
    text-align: left;
    color: var(--text-2);
    background: transparent;
    border: none;
    border-radius: var(--r-xs);
    cursor: default;
  }
  .line:hover,
  .line:focus-visible {
    color: var(--text);
    background: var(--s3);
  }
  .line[aria-current="true"] {
    color: var(--text);
    background: var(--sel);
  }
  .results:focus-within .line[aria-current="true"] {
    background: var(--sel-focus);
  }
  .line:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
  .num {
    flex: none;
    min-width: 3ch;
    text-align: right;
    color: var(--text-2);
  }
  .text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: pre;
  }
  mark {
    color: var(--text);
    background: var(--warn-soft);
    border-radius: var(--radius-xs);
  }
  .more {
    padding: 4px 8px;
    font: inherit;
    color: var(--text-2);
    background: transparent;
    border: 0;
    border-radius: var(--radius-sm);
    cursor: pointer;
  }
  .more:hover {
    color: var(--text);
    background: var(--s3);
  }
  @media (forced-colors: active) {
    input {
      border-color: FieldText;
    }
    mark {
      forced-color-adjust: none;
      background: Mark;
      color: MarkText;
    }
  }
</style>
