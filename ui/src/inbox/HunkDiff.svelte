<script lang="ts">
// One file's diff in Reviews, hunk by hunk: each hunk can be marked kept,
// discarded from the agent's work, or commented on line by line. Nothing
// here acts on its own: discard is one call made for a person's press, and
// line comments wait in the draft until the person sends the comment.
import Code from "../editor/highlight/Code.svelte";
import { highlightLines, type Segment } from "../editor/highlight/static";
import { languageFor } from "../editor/languages";
import type { Hunk, LineComment } from "../lib/protocol";
import PatchView from "../viewers/PatchView.svelte";
import { commentLine, type HunkLine, hunkPlace, type ParsedHunk, parseHunks } from "./hunks";
import type { Step } from "./steps";

/** Longer diffs are shown whole, without per-part actions. */
const MAX_LINES = 2000;

let {
  patch,
  path,
  discard,
  busy,
  kept,
  comments,
  onkeep,
  ondiscard,
  oncomment,
  onuncomment,
  stepOf = () => null,
}: {
  patch: string;
  path: string;
  /** Whether parts can be discarded; or why not, in words. */
  discard: true | string;
  busy: boolean;
  /** Keys of the hunks marked kept. */
  kept: ReadonlySet<string>;
  /** The draft's line comments on this file. */
  comments: readonly LineComment[];
  onkeep: (key: string) => void;
  ondiscard: (hunk: Hunk) => void;
  oncomment: (comment: LineComment) => void;
  onuncomment: (comment: LineComment) => void;
  /** The agent's step that wrote a part, when it can be told. */
  stepOf?: (part: ParsedHunk) => Step | null;
} = $props();

function stepText(step: Step): string {
  const said = step.said ? `: “${step.said}”` : "";
  return `Step ${step.number} (${step.tool})${said}`;
}

const hunks = $derived(parseHunks(patch));
const size = $derived(hunks.reduce((n, h) => n + h.lines.length, 0));
const shownWhole = $derived(hunks.length === 0 || size > MAX_LINES);

/** Syntax colour per line, the file highlighted as one text once the worker answers. */
let painted = $state.raw(new WeakMap<HunkLine, Segment[]>());
$effect(() => {
  const lines = hunks.flatMap((h) => h.lines.filter((l) => l.kind !== "note"));
  let live = true;
  void highlightLines(
    languageFor(path.split("/").pop() ?? path),
    lines.map((l) => l.text),
  ).then((segments) => {
    if (!live) return;
    const next = new WeakMap<HunkLine, Segment[]>();
    lines.forEach((line, i) => {
      const found = segments?.[i];
      if (found) next.set(line, found);
    });
    painted = next;
  });
  return () => {
    live = false;
  };
});

/** The line a comment is being written for, and in which hunk. */
let writing = $state<{ hunk: string; line: number; old: boolean } | null>(null);
let draft = $state("");

function lineKey(c: { line: number; old?: boolean }): string {
  return `${c.old ? "old" : "new"}:${c.line}`;
}

function open(hunk: string, at: { line: number; old: boolean }): void {
  writing = { hunk, ...at };
  draft = comments.find((c) => lineKey(c) === lineKey(at))?.text ?? "";
  queueMicrotask(() => document.getElementById(`line-comment-${lineKey(at)}`)?.focus());
}

function save(): void {
  if (!writing || !draft.trim()) return;
  const old = comments.find((c) => lineKey(c) === lineKey(writing ?? { line: 0, old: false }));
  if (old) onuncomment(old);
  oncomment({ path, line: writing.line, ...(writing.old ? { old: true } : {}), text: draft.trim() });
  writing = null;
  draft = "";
}

function onDraftKeydown(event: KeyboardEvent): void {
  if (event.code === "Enter" && (event.ctrlKey || event.metaKey)) {
    event.preventDefault();
    save();
  } else if (event.code === "Escape") {
    // Leaves the comment, not the whole view.
    event.preventDefault();
    event.stopPropagation();
    writing = null;
  }
}

/** The first changed line of a hunk: where its Comment button starts. */
function firstChanged(lines: HunkLine[]): { line: number; old: boolean } | null {
  const line = lines.find((l) => l.kind === "added" || l.kind === "removed") ?? lines[0];
  return line ? commentLine(line) : null;
}

const SIGN = { context: " ", added: "+", removed: "-" } as const;
const LABEL = { context: "unchanged", added: "added", removed: "removed" } as const;
</script>

{#if shownWhole}
  {#if hunks.length > 0}
    <p class="note" data-testid="hunks-too-long">
      This diff is too long to keep or discard by part; keep or discard the whole review.
    </p>
  {/if}
  <PatchView text={patch} />
{:else}
  <div class="hunks" data-testid="hunk-diff">
    {#if discard !== true}
      <p class="note" data-testid="hunks-why">{discard}</p>
    {/if}
    {#each hunks as part, index (part.key)}
      {@const isKept = kept.has(part.key)}
      {@const place = hunkPlace(part)}
      {@const step = stepOf(part)}
      <section class="hunk" class:kept={isKept} aria-label="Part {index + 1} of {hunks.length}, {place}" data-testid="hunk">
        <!-- One part: Keep, Discard and Comment would repeat the review's own; lines still take comments. -->
        {#if hunks.length > 1}
        <header>
          <span class="place">Part {index + 1} of {hunks.length}: {place}</span>
          {#if part.hunk.header}<span class="where">{part.hunk.header}</span>{/if}
          <span class="spacer"></span>
          {#if isKept}<span class="state" data-testid="hunk-kept">Looks good</span>{/if}
          <button type="button" aria-pressed={isKept} data-testid="hunk-keep" onclick={() => onkeep(part.key)}
            >{isKept ? "Show again" : "Keep"}</button
          >
          {#if discard === true}
            <button
              type="button"
              class="danger"
              data-testid="hunk-discard"
              aria-disabled={busy}
              aria-label="Discard part {index + 1}, {place}"
              onclick={() => {
                if (!busy) ondiscard(part.hunk);
              }}>Discard</button
            >
          {/if}
          <button
            type="button"
            data-testid="hunk-comment"
            onclick={() => {
              const at = firstChanged(part.lines);
              if (at) open(part.key, at);
            }}>Comment</button
          >
        </header>
        {/if}
        <!-- A lone part's step is named only when the agent said what it did. -->
        {#if step && (hunks.length > 1 || step.said)}
          <p class="made" data-testid="hunk-step">{stepText(step)}</p>
        {/if}
        {#if !isKept || writing?.hunk === part.key}
          <div class="lines" role="list" aria-label="Lines of part {index + 1}">
            {#each part.lines as line, l (l)}
              {#if line.kind === "note"}
                <div class="row note-line" role="listitem">{line.text}</div>
              {:else}
                {@const at = commentLine(line)}
                {@const said = at ? comments.find((c) => lineKey(c) === lineKey(at)) : undefined}
                <div class="row {line.kind}" role="listitem">
                  <button
                    type="button"
                    class="no"
                    tabindex="-1"
                    title="Comment on this line"
                    aria-label="Comment on {LABEL[line.kind]} line {at?.line}"
                    onclick={() => at && open(part.key, at)}
                  >
                    <span>{"old" in line ? line.old : ""}</span>
                    <span>{"new" in line ? line.new : ""}</span>
                  </button>
                  <span class="sign" aria-hidden="true">{SIGN[line.kind]}</span>
                  <span class="visually-hidden">{LABEL[line.kind]} line {at?.line}:</span>
                  <span class="code"><Code text={line.text} segments={painted.get(line)} /></span>
                </div>
                {#if said && !(writing && writing.hunk === part.key && lineKey(writing) === lineKey(said))}
                  <div class="said" role="listitem" data-testid="line-comment">
                    <span>{said.text}</span>
                    <button type="button" onclick={() => at && open(part.key, at)}>Edit</button>
                    <button type="button" onclick={() => onuncomment(said)}>Remove</button>
                  </div>
                {/if}
                {#if writing && at && writing.hunk === part.key && lineKey(writing) === lineKey(at)}
                  <form
                    class="write"
                    role="listitem"
                    onsubmit={(e) => {
                      e.preventDefault();
                      save();
                    }}
                  >
                    <label for="line-comment-{lineKey(at)}">
                      Comment on {at.old ? "removed line" : "line"}
                      {at.line}, sent with your next comment to the agent
                    </label>
                    <select
                      aria-label="Line"
                      value={lineKey(at)}
                      onchange={(e) => {
                        const [side, no] = e.currentTarget.value.split(":");
                        open(part.key, { line: Number(no), old: side === "old" });
                      }}
                    >
                      {#each part.lines as other, o (o)}
                        {@const there = commentLine(other)}
                        {#if there}
                          <option value={lineKey(there)}
                            >{there.old ? "Removed line" : "Line"} {there.line}: {other.text.slice(0, 40)}</option
                          >
                        {/if}
                      {/each}
                    </select>
                    <textarea
                      id="line-comment-{lineKey(at)}"
                      rows="2"
                      bind:value={draft}
                      aria-keyshortcuts="Control+Enter Meta+Enter"
                      onkeydown={onDraftKeydown}
                    ></textarea>
                    <div class="row-actions">
                      <button type="submit" aria-disabled={!draft.trim()} data-testid="line-comment-add">Add comment</button>
                      <button type="button" onclick={() => (writing = null)}>Cancel</button>
                    </div>
                  </form>
                {/if}
              {/if}
            {/each}
          </div>
        {/if}
      </section>
    {/each}
  </div>
{/if}

<style>
  .hunks {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 8px 12px 16px;
    overflow: auto;
    height: 100%;
    box-sizing: border-box;
  }
  .note {
    margin: 0;
    padding: 6px 12px;
    color: var(--text-2);
    font-size: var(--fs-ui);
  }
  .hunks > .note {
    padding: 0;
  }
  .hunk {
    border: 1px solid var(--separator);
    border-radius: var(--radius);
    overflow: hidden;
  }
  .hunk.kept {
    opacity: 0.8;
  }
  header {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 8px;
    padding: 4px 8px;
    background: var(--s2);
    font-size: var(--fs-ui);
  }
  .place {
    font-weight: 600;
  }
  .made {
    margin: 0;
    padding: 2px 8px 4px;
    overflow: hidden;
    color: var(--text-2);
    font-size: var(--fs-ui-sm);
    white-space: nowrap;
    text-overflow: ellipsis;
    background: var(--s2);
  }
  .where {
    color: var(--text-2);
    font-family: var(--mono);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 40ch;
  }
  .spacer {
    flex: 1;
  }
  .state {
    color: var(--ok);
  }
  header button,
  .said button,
  .row-actions button {
    min-height: var(--control-h-sm);
    padding: 2px 10px;
    font: inherit;
    color: var(--text);
    background: var(--s3);
    border: 0;
    border-radius: var(--r-control);
    cursor: pointer;
  }
  header button:hover,
  .said button:hover,
  .row-actions button:hover {
    background: var(--s4);
  }
  header button.danger {
    color: var(--danger);
  }
  button[aria-disabled="true"] {
    opacity: 0.55;
    cursor: default;
  }
  .lines {
    font-family: var(--mono);
    font-size: var(--fs-ui-sm);
  }
  .row {
    display: flex;
    align-items: baseline;
    min-height: 20px;
    border-left: 3px solid transparent;
    white-space: pre;
  }
  .added {
    background: var(--ok-soft);
    border-left-color: var(--ok);
  }
  .removed {
    background: var(--danger-soft);
    border-left-color: var(--danger);
  }
  .note-line {
    color: var(--text-2);
    padding-left: 12ch;
  }
  .no {
    all: unset;
    display: inline-flex;
    flex: none;
    cursor: pointer;
    color: var(--text-2);
    user-select: none;
  }
  .no span {
    display: inline-block;
    width: 5ch;
    padding-right: 6px;
    text-align: right;
  }
  .no:hover {
    color: var(--accent);
  }
  .sign {
    display: inline-block;
    flex: none;
    width: 2ch;
    text-align: center;
    user-select: none;
  }
  .code {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .said,
  .write {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 8px;
    margin: 2px 8px 6px 12ch;
    padding: 6px 8px;
    border-left: 3px solid var(--accent);
    background: var(--accent-soft);
    border-radius: var(--radius-sm);
    font-family: var(--font);
    font-size: var(--fs-ui);
    white-space: normal;
  }
  .said span {
    flex: 1;
    min-width: 0;
    white-space: pre-wrap;
  }
  .write {
    flex-direction: column;
    align-items: stretch;
  }
  .write select {
    align-self: flex-start;
    max-width: 100%;
    font: inherit;
    background: var(--s3);
    color: var(--text);
    border: 0;
    border-radius: var(--r-control);
    padding: 2px 6px;
  }
  .write textarea {
    font: inherit;
    resize: vertical;
  }
  .row-actions {
    display: flex;
    gap: 8px;
  }
  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
  @media (forced-colors: active) {
    .added {
      border-left: 3px solid CanvasText;
    }
    .removed {
      border-left: 3px dashed CanvasText;
    }
  }
</style>
