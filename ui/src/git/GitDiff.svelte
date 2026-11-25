<script lang="ts">
// One file's git diff in the main area, unstaged (index to working tree) or
// staged (HEAD to index), with "Stage hunk" or "Unstage hunk" on each hunk of
// a modified file. It reloads when the panel's model sees a change, so the
// panel's calls, this view's calls and `fs.git_changed` all keep it current.
// It is the selected tab while it shows: Escape and the close-tab shortcut
// close it, next / previous tab go to the file tabs, and closing puts focus
// back on the panel row it was opened from.
import { tick } from "svelte";
import { focusMain, show, tabs } from "../editor/open-files.svelte";
import { listCommands } from "../keys/commands";
import { inBrowserTab, isApple, liveKeys, matchesShortcut, parseShortcut } from "../keys/shortcut";
import type { FileDiff, Hunk } from "../lib/protocol";
import { useOvra } from "../shell/context";
import type { GitDiffTarget } from "../shell/workspace.svelte";
import { focusRow } from "./focus";
import { gitModel, message } from "./model.svelte";

let { target }: { target: GitDiffTarget } = $props();
const { transport, workspace } = useOvra();

/** Diff lines rendered at first and added by "Show more". */
const PAGE = 2000;

const model = $derived(gitModel(target.project));
const which = $derived(`${target.path}\n${target.staged}\n`);
const key = $derived(`${which}${model?.version ?? 0}`);
let loaded = $state<{ key: string; data: FileDiff | null; error: string | null } | null>(null);
let shownLines = $state(PAGE);
let announcement = $state("");
let heading = $state<HTMLElement>();
let view = $state<HTMLElement>();
/** The hunk whose button was pressed, to keep focus there once the diff reloads. */
let refocus: number | null = null;

/** The diff shown: the latest load of this file and side; an older one stays up while it reloads. */
const shown = $derived(loaded?.key.startsWith(which) ? loaded : null);
const file = $derived(
  model?.state.kind === "ready" ? model.state.status.files.find((f) => f.path === target.path) : undefined,
);
const change = $derived(target.staged ? file?.staged : file?.unstaged);
const hunks = $derived(shown?.data?.hunks ?? []);
/** Why hunks cannot be staged one by one, or null when they can. */
const noHunks = $derived.by(() => {
  const data = shown?.data;
  if (!data || data.binary || hunks.length === 0) return null;
  if (data.truncated) return "This diff is too long to stage by hunk: stage the whole file from the Git panel.";
  if (change === "untracked") return "This file is new and untracked: stage the whole file from the Git panel.";
  if (change !== "modified") return "Hunks of an added, deleted or renamed file go with the whole file.";
  return null;
});
/** Hunks rendered: all whose first line is within `shownLines`. */
const visible = $derived.by(() => {
  const out: Hunk[] = [];
  let lines = 0;
  for (const hunk of hunks) {
    if (lines >= shownLines) break;
    out.push(hunk);
    lines += hunk.lines.length;
  }
  return out;
});
const hiddenLines = $derived(hunks.slice(visible.length).reduce((sum, h) => sum + h.lines.length, 0));

$effect(() => {
  const mine = key;
  const { project, path, staged } = target;
  let live = true;
  transport.call({ method: "git.diff", params: { project, path, staged } }).then(
    (reply) => {
      if (live) loaded = { key: mine, data: reply.data, error: null };
      void keepFocus();
    },
    (err: unknown) => {
      if (live) loaded = { key: mine, data: null, error: message(err) };
    },
  );
  return () => {
    live = false;
  };
});

// A newly chosen file or side: start at the top, with focus on the heading.
$effect(() => {
  void which;
  shownLines = PAGE;
  void tick().then(() => heading?.focus());
});

/** After staging a hunk, focus the button now in its place, or the heading when none is left. */
async function keepFocus(): Promise<void> {
  if (refocus === null) return;
  const at = refocus;
  refocus = null;
  await tick();
  if (view?.contains(document.activeElement) && document.activeElement !== view) return;
  const buttons = [...(view?.querySelectorAll<HTMLElement>("[data-hunk-action]") ?? [])];
  (buttons[Math.min(at, buttons.length - 1)] ?? heading)?.focus();
}

async function stageHunk(hunk: Hunk, index: number): Promise<void> {
  if (!model || model.busy) return;
  refocus = index;
  const ok = await model.stageHunk(target.path, hunk, target.staged);
  // Said once, here: the panel stays quiet about hunks, and a failure is in the alert below.
  announcement = ok ? (target.staged ? "Hunk unstaged." : "Hunk staged.") : "";
}

/** Closes the diff; focus goes back to the panel row (see the effect below). */
function close(): void {
  workspace.gitDiff = null;
}

/** Opens the file; focus moves into the main area, which shows it. */
function openFile(): void {
  document.getElementById("ovra-tabpanel")?.focus();
  workspace.openFile(target.path);
  void focusMain();
}

// The diff last shown, for the focus to return to its row once it closes.
let shownTarget: GitDiffTarget | null = null;
$effect.pre(() => {
  shownTarget = target;
});
$effect(() => () => {
  const closed = shownTarget;
  void tick().then(() => {
    const active = document.activeElement;
    if (!closed || (active && active !== document.body && active.isConnected)) return;
    if (!focusRow(closed.path, closed.staged)) void focusMain();
  });
});

/** Which tab command `event` is, by the shortcuts the editor registered. */
function tabCommand(event: KeyboardEvent): string | undefined {
  const ids = ["editor.close_tab", "editor.next_tab", "editor.previous_tab"];
  return listCommands().find((command) => {
    if (!command.keys || !ids.includes(command.id)) return false;
    const shortcut = parseShortcut(liveKeys(command.keys, inBrowserTab()), isApple());
    return shortcut !== null && matchesShortcut(event, shortcut);
  })?.id;
}

// Before the window's shortcuts: while the diff is the selected tab, tab keys act on it, not on the file under it.
$effect(() => {
  const onkeydown = (event: KeyboardEvent): void => {
    if (event.defaultPrevented || event.isComposing) return;
    const active = document.activeElement;
    const inDiff = !!active && (view?.contains(active) || active.matches("[data-diff-tab]"));
    const command = event.code === "Escape" && inDiff ? "editor.close_tab" : tabCommand(event);
    if (!command) return;
    event.preventDefault();
    if (command === "editor.close_tab") {
      close();
      return;
    }
    // The diff is the last tab: next is the first file, previous the last one.
    const files = tabs.current?.files ?? [];
    const next = command === "editor.next_tab" ? files[0] : files.at(-1);
    if (!next) return;
    document.querySelector<HTMLElement>(`[role="tab"][data-path="${CSS.escape(next)}"]`)?.focus();
    show(workspace, next);
  };
  document.addEventListener("keydown", onkeydown);
  return () => document.removeEventListener("keydown", onkeydown);
});

/** Old and new line numbers of each line of `hunk`. */
function numbered(hunk: Hunk): { text: string; kind: string; old: number | null; new: number | null }[] {
  let old = hunk.old_start;
  let now = hunk.new_start;
  return hunk.lines.map((text) => {
    const sign = text[0];
    if (sign === "+") return { text, kind: "added", old: null, new: now++ };
    if (sign === "-") return { text, kind: "removed", old: old++, new: null };
    if (sign === "\\") return { text, kind: "note", old: null, new: null };
    return { text, kind: "context", old: old++, new: now++ };
  });
}

const range = (hunk: Hunk) =>
  hunk.new_lines === 0
    ? `removed after line ${hunk.new_start}`
    : `lines ${hunk.new_start} to ${hunk.new_start + hunk.new_lines - 1}`;
</script>

<section class="diff" aria-labelledby="git-diff-title" data-testid="git-diff" bind:this={view}>
  <header>
    <h2 id="git-diff-title" tabindex="-1" bind:this={heading} title={target.path}>
      <span class="path">{target.path}</span>
      <span class="side">{target.staged ? "Staged changes" : "Unstaged changes"}</span>
    </h2>
    <div class="buttons">
      <button type="button" onclick={() => (workspace.gitDiff = { ...target, staged: !target.staged })}
        >{target.staged ? "Show unstaged changes" : "Show staged changes"}</button
      >
      <button type="button" onclick={openFile}>Open file</button>
      <button type="button" onclick={close}>Close diff</button>
    </div>
  </header>
  <p class="visually-hidden" role="status">{announcement}</p>
  {#if model?.error?.scope === "diff"}
    <p class="error" role="alert">{model.error.message}</p>
  {/if}
  {#if !shown}
    <p class="hint">Loading the diff…</p>
  {:else if shown.error}
    <p class="error" role="alert">The diff did not load: {shown.error}</p>
  {:else if shown.data?.binary}
    <p class="hint">Binary file: there is no text diff to show.</p>
  {:else if hunks.length === 0}
    <p class="hint">{target.staged ? "No staged changes in this file." : "No unstaged changes in this file."}</p>
  {:else}
    {#if noHunks}
      <p class="hint" data-testid="git-diff-no-hunks">{noHunks}</p>
    {/if}
    {#if shown.data?.truncated}
      <p class="hint">Only the first 20,000 lines of this diff are shown.</p>
    {/if}
    <div class="hunks">
      {#each visible as hunk, i (i)}
        <section class="hunk" aria-label={`Hunk ${i + 1} of ${hunks.length}, ${range(hunk)}`}>
          <div class="hunk-head">
            <code
              >@@ -{hunk.old_start},{hunk.old_lines} +{hunk.new_start},{hunk.new_lines} @@{hunk.header
                ? ` ${hunk.header}`
                : ""}</code
            >
            {#if !noHunks}
              <button
                type="button"
                data-hunk-action
                aria-label={`${target.staged ? "Unstage" : "Stage"} hunk ${i + 1}, ${range(hunk)}`}
                aria-disabled={model?.busy}
                onclick={() => stageHunk(hunk, i)}>{target.staged ? "Unstage hunk" : "Stage hunk"}</button
              >
            {/if}
          </div>
          <div class="lines">
            {#each numbered(hunk) as line, j (j)}
              <div class="line {line.kind}">
                <span class="num" aria-hidden="true">{line.old ?? ""}</span>
                <span class="num" aria-hidden="true">{line.new ?? ""}</span>
                <span class="text">{line.text}</span>
              </div>
            {/each}
          </div>
        </section>
      {/each}
      {#if hiddenLines > 0}
        <button type="button" class="more" onclick={() => (shownLines += PAGE)}
          >Show more ({hiddenLines.toLocaleString("en-US")} lines hidden)</button
        >
      {/if}
    </div>
  {/if}
</section>

<style>
  .diff {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    box-sizing: border-box;
  }
  header {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 12px;
    padding: 6px 12px;
    border-bottom: 1px solid #2a2a31;
  }
  h2 {
    display: flex;
    gap: 8px;
    align-items: baseline;
    flex: 1;
    min-width: 0;
    margin: 0;
    font-size: 13px;
    font-weight: normal;
  }
  .path {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .side {
    flex: none;
    font-size: 12px;
    color: #9a9aa6;
  }
  .buttons {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  button {
    padding: 2px 8px;
    font: inherit;
    font-size: 12px;
    color: #e6e6e6;
    background: #24242c;
    border: 1px solid #3a3a44;
    border-radius: 3px;
    cursor: pointer;
  }
  button:focus-visible,
  h2:focus-visible {
    outline: 2px solid #4a6bb0;
    outline-offset: 1px;
  }
  button[aria-disabled="true"] {
    opacity: 0.6;
    cursor: default;
  }
  .hint,
  .error {
    margin: 8px 12px;
    color: #9a9aa6;
  }
  .error {
    color: #ff6b6b;
  }
  .hunks {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 0 0 12px;
  }
  .hunk-head {
    position: sticky;
    top: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 12px;
    background: #1c2230;
  }
  .hunk-head code {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: #9fb3d9;
  }
  .lines {
    font-family: ui-monospace, monospace;
    font-size: 12px;
  }
  .line {
    display: flex;
    white-space: pre;
  }
  .num {
    flex: none;
    width: 5ch;
    padding-right: 6px;
    text-align: right;
    color: #6f6f7a;
  }
  .text {
    padding-left: 4px;
  }
  .added {
    background: #1d3324;
  }
  .removed {
    background: #3a1f22;
  }
  .note {
    color: #9a9aa6;
  }
  .more {
    margin: 8px 12px;
  }
  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
</style>
