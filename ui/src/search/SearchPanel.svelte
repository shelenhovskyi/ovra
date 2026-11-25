<script lang="ts">
// Project-wide content search: a query with case, word and regex toggles and
// include/exclude globs; results grouped by file. Choosing a result opens the
// file in the editor with the match selected.
import type { FileMatches, LineMatch, ProjectId } from "../lib/protocol";
import type { Transport } from "../lib/transport";
import type { TextSpot } from "../shell/workspace.svelte";
import { SearchModel } from "./model.svelte";

let {
  transport,
  project,
  onopen,
}: {
  transport: Transport;
  project: ProjectId;
  onopen: (path: string, at: TextSpot) => void;
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

/** Focuses the search text with it selected. */
export function focus(): void {
  input?.focus();
  input?.select();
}

/** Files to render, cut so that at most `shownLines` lines are in the DOM. */
const visible = $derived.by(() => {
  const out: FileMatches[] = [];
  let left = shownLines;
  for (const file of model.files) {
    if (left <= 0) break;
    out.push(file.lines.length <= left ? file : { path: file.path, lines: file.lines.slice(0, left) });
    left -= file.lines.length;
  }
  return out;
});

const plural = (n: number, one: string, many: string) => `${n.toLocaleString("en-US")} ${n === 1 ? one : many}`;

const summary = $derived.by(() => {
  const status = model.status;
  const found = `${plural(model.lineCount, "matching line", "matching lines")} in ${plural(model.files.length, "file", "files")}`;
  switch (status.kind) {
    case "idle":
      return "";
    case "error":
      return status.message;
    case "searching":
      return model.files.length ? `Searching… ${found} so far` : "Searching…";
    case "done":
      if (status.stopped) return model.files.length ? `Stopped: ${found} so far.` : "Stopped.";
      if (!model.files.length) return "No results.";
      return status.truncated ? `${found}. Limit reached: narrow the search.` : `${found}.`;
  }
  return "";
});

function changed(): void {
  shownLines = PAGE;
  model.schedule();
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
    void model.run();
  } else if (event.key === "Escape" && model.status.kind === "searching") {
    event.preventDefault();
    model.stop();
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

/** Characters kept before the first match when a line is shortened for the narrow sidebar. */
const LEAD = 12;

/**
 * The line's text cut into plain and matched parts. Indentation is dropped, and a
 * line whose first match starts far in begins shortly before it, after "…".
 */
function parts(match: LineMatch): { text: string; hit: boolean }[] {
  const first = match.ranges[0]?.[0] ?? 0;
  const indent = match.text.length - match.text.trimStart().length;
  const start = Math.min(first, first - indent > 2 * LEAD ? first - LEAD : indent);
  const out: { text: string; hit: boolean }[] = [];
  if (start > indent || match.column > 0) out.push({ text: "…", hit: false });
  let at = start;
  for (const [from, to] of match.ranges) {
    if (to <= at) continue;
    if (from > at) out.push({ text: match.text.slice(at, from), hit: false });
    out.push({ text: match.text.slice(Math.max(from, at), to), hit: true });
    at = to;
  }
  if (at < match.text.length) out.push({ text: match.text.slice(at), hit: false });
  return out;
}

function open(path: string, match: LineMatch): void {
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
        onclick={() => (showFilters = !showFilters)}>⋯</button
      >
    </div>
  </div>
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
        </div>
        <ul>
          {#each file.lines as match (match.line)}
            <li>
              <button
                type="button"
                class="line"
                data-result
                tabindex={resultKey(file.path, match) === stop ? 0 : -1}
                onfocus={() => (current = resultKey(file.path, match))}
                aria-label={`${file.path} line ${match.line}: ${match.text.trim()}`}
                onclick={() => open(file.path, match)}
              >
                <span class="num" aria-hidden="true">{match.line}</span>
                <span class="text" aria-hidden="true"
                  >{#each parts(match) as part, i (i)}{#if part.hit}<mark>{part.text}</mark
                      >{:else}{part.text}{/if}{/each}</span
                >
              </button>
            </li>
          {/each}
        </ul>
      </li>
    {/each}
  </ul>
  {#if model.lineCount > shownLines}
    <button type="button" class="more" onclick={() => (shownLines += PAGE)}>
      Show {plural(model.lineCount - shownLines, "more line", "more lines")}
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
  label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 13px;
    color: var(--text-2);
  }
  .summary,
  .keys {
    margin: 0;
    font-size: 13px;
    color: var(--text-2);
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
    align-items: baseline;
    padding: 6px 4px 2px;
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
    border-radius: 9px;
    color: var(--text-2);
    background: var(--s3);
  }
  .line {
    display: flex;
    gap: 8px;
    width: 100%;
    padding: 3px 4px 3px 8px;
    font: 13px var(--mono);
    text-align: left;
    color: var(--text-2);
    background: transparent;
    border: none;
    border-radius: var(--radius-sm);
    cursor: pointer;
  }
  .line:hover,
  .line:focus-visible {
    color: var(--text);
    background: var(--s3);
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
    border-radius: 3px;
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
