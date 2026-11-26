<script lang="ts">
// "Open pull request…": what would be sent, from `pr.prepare`, and the
// person's press that sends it (docs/specs/pr-flow.md). The worker drafted
// the title and body; the person edits them, the base and the branch.
// Without a usable `gh` it only says how to get one; with conflicts it
// offers to ask the worker to update instead.
import { onMount, tick } from "svelte";
import type { PrPrepared, ReviewItem } from "../lib/protocol";
import { openTerminalIn } from "../terminal/open-in";
import { fileSummary } from "./describe";
import { ghAccount, ghProblem } from "./pull-request";
import { message as errorText, type ReviewInbox } from "./reviews.svelte";

let {
  inbox,
  item,
  onclose,
}: {
  inbox: ReviewInbox;
  item: ReviewItem;
  /** With the text to announce, when something was sent. */
  onclose: (said?: string) => void;
} = $props();

let dialog = $state<HTMLDialogElement>();
let prepared = $state<PrPrepared | null>(null);
let error = $state<string | null>(null);
let checking = $state(false);
let sending = $state(false);
let base = $state("");
let branch = $state("");
let title = $state("");
let body = $state("");
let draft = $state(true);
/** The files the person looked at in Reviews: what Create sends. */
// svelte-ignore state_referenced_locally
const reviewed = item.fingerprint;

const problem = $derived(prepared ? ghProblem(prepared.gh) : null);
const account = $derived(prepared ? ghAccount(prepared.gh) : null);
/** The worker's files moved on since the person looked at them in Reviews. */
const moved = $derived(prepared !== null && prepared.fingerprint !== reviewed);
const missing = $derived(
  [!title.trim() && "a title", !base.trim() && "a base branch", !branch.trim() && "a branch name"].filter(
    (m): m is string => typeof m === "string",
  ),
);
const canCreate = $derived(
  prepared !== null && !problem && prepared.conflicts.length === 0 && !moved && missing.length === 0,
);

onMount(() => {
  dialog?.showModal();
  void load(null, true);
});

/** Asks what would be sent; `fill` also takes the drafted fields. */
async function load(asked: string | null, fill: boolean): Promise<void> {
  checking = true;
  try {
    const next = await inbox.preparePr(item.id, asked);
    prepared = next;
    error = null;
    if (fill) {
      ({ base, branch, title, body } = next);
      await tick();
      dialog?.querySelector<HTMLElement>("[data-first]")?.focus();
    }
  } catch (err) {
    error = errorText(err);
  } finally {
    checking = false;
  }
}

/** A new base changes what conflicts: ask again, keeping the person's edits. */
function baseChanged(): void {
  if (prepared && base.trim() && base.trim() !== prepared.base) void load(base.trim(), false);
}

async function create(): Promise<void> {
  if (!prepared || !canCreate || sending) return;
  sending = true;
  const sent = {
    remote: prepared.remote,
    base: base.trim(),
    branch: branch.trim(),
    title: title.trim(),
    body,
    draft,
  };
  const pr = await inbox.createPr(item.id, sent, reviewed);
  sending = false;
  if (pr) onclose(`Pull request #${pr.number} opened.`);
  else error = inbox.error;
}

async function askUpdate(): Promise<void> {
  if (!prepared || sending) return;
  sending = true;
  const asked = await inbox.askUpdate(item.id, prepared.remote, prepared.base);
  sending = false;
  if (asked) onclose("Asked the worker to update. Review its changes again when it is done.");
  else error = inbox.error;
}

function onkeydown(event: KeyboardEvent): void {
  if (event.code === "Escape") {
    // The dialog's own: Reviews behind it stays open.
    event.preventDefault();
    event.stopPropagation();
    onclose();
  } else if (event.code === "Enter" && (event.ctrlKey || event.metaKey)) {
    event.preventDefault();
    void create();
  }
}
</script>

<dialog
  class="pr"
  aria-labelledby="pr-dialog-title"
  aria-describedby="pr-dialog-what"
  data-testid="pr-dialog"
  bind:this={dialog}
  oncancel={(event) => {
    event.preventDefault();
    onclose();
  }}
  {onkeydown}
>
  <h2 id="pr-dialog-title">Open pull request</h2>
  <p class="hint" id="pr-dialog-what">
    Ovra commits exactly what you reviewed onto the base branch and pushes it with your own GitHub CLI.
  </p>
  {#if error}<p class="error" role="alert">{error}</p>{/if}

  {#if !prepared}
    {#if checking}<p class="hint" role="status">Getting ready…</p>{/if}
  {:else}
    {#if problem}
      <div class="warn" role="note" data-testid="pr-no-gh">
        <p>{problem}</p>
        <button type="button" data-first onclick={() => {
            openTerminalIn("");
            onclose();
          }}>Open terminal</button
        >
      </div>
    {:else}
      <p class="where" data-testid="pr-where">
        To <span class="mono">{prepared.remote}</span>{prepared.host ? ` (${prepared.host})` : ""}{account
          ? ` · ${account}`
          : ""}
      </p>
    {/if}

    <div class="row">
      <label class="field">
        Base branch
        <input type="text" class="mono" bind:value={base} onchange={baseChanged} spellcheck="false" />
      </label>
      <label class="field">
        New branch
        <input type="text" class="mono" bind:value={branch} spellcheck="false" />
      </label>
    </div>

    <ul class="files" aria-label="Files in the pull request">
      {#each prepared.files as file (file.path)}
        <li><span class="mono">{file.path}</span> <span class="hint">{fileSummary(file)}</span></li>
      {/each}
    </ul>

    {#if prepared.conflicts.length > 0}
      <div class="warn" role="note" data-testid="pr-conflicts">
        <p>
          These files conflict with {prepared.remote}/{prepared.base}: {prepared.conflicts.join(", ")}. The worker
          must merge it first.
        </p>
        <button type="button" aria-disabled={sending} onclick={askUpdate}>Ask the worker to update</button>
      </div>
    {/if}
    {#if moved}
      <p class="warn" role="note">The worker's files changed since you looked. Close this and review them again.</p>
    {/if}

    {#if !problem}
      <label class="field">
        Title
        <input type="text" bind:value={title} data-first />
      </label>
      <label class="field">
        Description
        <textarea rows="5" bind:value={body}></textarea>
      </label>
      <label class="check">
        <input type="checkbox" bind:checked={draft} />
        Draft
      </label>
    {/if}
  {/if}

  <div class="buttons">
    {#if prepared && !problem && prepared.conflicts.length === 0}
      <button
        type="button"
        class="primary"
        aria-disabled={!canCreate || sending}
        aria-describedby={missing.length > 0 ? "pr-dialog-missing" : undefined}
        onclick={create}>{sending ? "Creating…" : "Create pull request"}</button
      >
    {/if}
    <button type="button" onclick={() => onclose()}>Cancel</button>
  </div>
  {#if prepared && !problem && missing.length > 0}
    <p class="hint" id="pr-dialog-missing">Needs {missing.join(", ")}.</p>
  {/if}
</dialog>

<style>
  dialog {
    width: min(560px, calc(100vw - 48px));
    max-height: calc(100vh - 48px);
    padding: 20px 24px;
    color: var(--text);
    background: var(--s4);
    border: 0;
    border-radius: var(--radius-lg);
    box-shadow: var(--card-shadow);
    font-size: 14px;
  }
  dialog::backdrop {
    background: color-mix(in srgb, var(--s0) 55%, transparent);
  }
  dialog > * + * {
    margin-top: 12px;
  }
  h2 {
    margin: 0;
    font-size: 16px;
    font-weight: 600;
  }
  p {
    margin: 0;
  }
  .hint,
  .where {
    color: var(--text-2);
    font-size: 13px;
  }
  .mono {
    font-family: var(--mono);
  }
  .error {
    color: var(--danger);
  }
  .warn {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 8px;
    padding: 8px 12px;
    color: var(--text);
    background: var(--warn-soft);
    border-radius: var(--radius);
  }
  .row {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
    color: var(--text-2);
    font-size: 13px;
  }
  input[type="text"],
  textarea {
    box-sizing: border-box;
    width: 100%;
    min-height: var(--control-h);
    padding: 6px 10px;
    font: inherit;
    font-size: 14px;
    color: var(--text);
    background: var(--s3);
    border: 0;
    border-radius: var(--radius-sm);
  }
  input.mono {
    font-family: var(--mono);
  }
  textarea {
    resize: vertical;
  }
  .files {
    max-height: 140px;
    overflow: auto;
    margin: 12px 0 0;
    padding: 0;
    list-style: none;
    font-size: 13px;
  }
  .files li {
    padding: 2px 0;
    overflow-wrap: anywhere;
  }
  .check {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  input[type="checkbox"] {
    accent-color: var(--accent);
  }
  .buttons {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
  }
  button {
    min-height: var(--control-h-sm);
    padding: 5px 12px;
    font: inherit;
    font-weight: 500;
    color: var(--text-2);
    background: transparent;
    border: 0;
    border-radius: var(--radius-sm);
    cursor: pointer;
  }
  button:hover {
    background: var(--s3);
  }
  .warn button {
    color: var(--text);
    background: var(--s3);
  }
  .primary,
  .primary:hover {
    min-height: var(--control-h);
    color: var(--accent-ink);
    background: var(--accent);
  }
  button[aria-disabled="true"] {
    opacity: 0.55;
    cursor: default;
  }
  button:focus-visible,
  input:focus-visible,
  textarea:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
  @media (forced-colors: active) {
    dialog,
    .warn {
      border: 1px solid CanvasText;
    }
    button[aria-disabled="true"] {
      color: GrayText;
    }
  }
</style>
