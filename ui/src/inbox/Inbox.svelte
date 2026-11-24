<script lang="ts">
// The review inbox: agents' changes waiting for a person. A header button
// (and Mod+Shift+R, Alt+Shift+R in a browser tab) opens a dialog with the
// project's review items, the files of the chosen one and that file's diff.
// Approve, reject and comment are always a person's press; nothing here
// approves or merges on its own. Keys: arrows move in a list, N / P move
// between changes in the diff, Mod+Enter sends a comment, Escape closes.
import { tick, untrack } from "svelte";
import { agentSessions, sessionName } from "../agents/sessions.svelte";
import { registerCommand } from "../keys/commands";
import { formatShortcut, inBrowserTab, isApple, liveKeys } from "../keys/shortcut";
import type { ReviewId, ReviewItem } from "../lib/protocol";
import { useOvra } from "../shell/context";
import PatchView from "../viewers/PatchView.svelte";
import {
  approveHint,
  defaultCommitMessage,
  fileMarkers,
  fileSummary,
  itemSummary,
  rejectHint,
  STATE_LABEL,
} from "./describe";
import { type Outcome, ReviewInbox } from "./reviews.svelte";

const KEYS = "Mod+Shift+R";
const { transport, workspace } = useOvra();
const inbox = new ReviewInbox(transport);
const sessions = agentSessions(transport);
const shortcut = formatShortcut(liveKeys(KEYS, inBrowserTab()), isApple());
const modEnter = formatShortcut("Mod+Enter", isApple());

let opened = $state(false);
let selectedId = $state<ReviewId | null>(null);
let selectedPath = $state<string | null>(null);
let diff = $state<{ key: string; patch: string | null; error: string | null } | null>(null);
let commit = $state(false);
let commitMessage = $state("");
let confirmingReject = $state(false);
let comment = $state("");
let announcement = $state("");
/** An agent asked for a review since the inbox was last open. */
let asked = $state(false);
let dialog = $state<HTMLElement>();
let returnTo: HTMLElement | null = null;

const item = $derived(inbox.items.find((i) => i.id === selectedId) ?? null);
const file = $derived(item?.files.find((f) => f.path === selectedPath) ?? null);
const busy = $derived(item ? inbox.busy.includes(item.id) : false);
const outcome = $derived(inbox.outcome && inbox.outcome.id === item?.id ? inbox.outcome : null);
const diffKey = $derived(item && file ? `${item.id}\n${file.path}\n${inbox.changes}` : null);

$effect(() => () => inbox.dispose());

$effect(() => {
  const project = workspace.project?.id ?? null;
  untrack(() => void inbox.show(project));
});

$effect(() => registerCommand({ id: "inbox.toggle", title: "Review inbox", keys: KEYS, run: toggle }));

// An agent asked to show a review (`open_in_ui`): choose it and say so on the
// button, but never open the dialog or move focus on an agent's behalf.
$effect(() =>
  transport.onEvent((event) => {
    if (event.type !== "orchestration.show" || event.data.target.kind !== "review") return;
    if (event.data.project !== workspace.project?.id) return;
    const id = event.data.target.id;
    void inbox.reload().then(() => {
      select(id);
      if (!opened) asked = true;
    });
  }),
);

// Keep a valid item and file selected as the list changes.
$effect(() => {
  const items = inbox.items;
  untrack(() => {
    if (!items.some((i) => i.id === selectedId)) select((inbox.open[0] ?? items[0])?.id ?? null);
    else if (item && !item.files.some((f) => f.path === selectedPath)) selectedPath = item.files[0]?.path ?? null;
  });
});

// Load the chosen file's diff, again after each change; the old text stays up while it reloads.
$effect(() => {
  const key = diffKey;
  if (!key) {
    diff = null;
    return;
  }
  const [id = "", path = ""] = key.split("\n");
  let live = true;
  untrack(() => {
    if (!diff?.key.startsWith(`${id}\n${path}\n`)) diff = { key, patch: null, error: null };
  });
  inbox.diff(id, path).then(
    (patch) => {
      if (live) diff = { key, patch, error: null };
    },
    (err: unknown) => {
      if (live) diff = { key, patch: null, error: err instanceof Error ? err.message : String(err) };
    },
  );
  return () => {
    live = false;
  };
});

function select(id: ReviewId | null): void {
  if (id === selectedId) return;
  selectedId = id;
  const next = inbox.items.find((i) => i.id === id);
  selectedPath = next?.files[0]?.path ?? null;
  commit = false;
  commitMessage = next ? defaultCommitMessage(next) : "";
  confirmingReject = false;
  comment = "";
}

async function show(): Promise<void> {
  const active = document.activeElement;
  returnTo = active instanceof HTMLElement ? active : null;
  opened = true;
  asked = false;
  await tick();
  const target =
    dialog?.querySelector<HTMLElement>("[data-item][aria-current='true']") ??
    dialog?.querySelector<HTMLElement>("[data-item]") ??
    dialog?.querySelector<HTMLElement>("h2");
  target?.focus();
}

function close(): void {
  opened = false;
  confirmingReject = false;
  if (returnTo?.isConnected) returnTo.focus();
  returnTo = null;
}

function toggle(): void {
  if (opened) close();
  else void show();
}

function sessionLabel(review: ReviewItem): string {
  const name = sessionName(sessions.byId.get(review.session), review.session);
  return `${review.mode === "direct" ? "Direct agent" : "Worker"}: ${name}`;
}

/** Arrow keys move focus (and the choice) within a list of buttons. */
function moveIn(event: KeyboardEvent, selector: string, pick: (el: HTMLElement) => void): void {
  if (event.code !== "ArrowDown" && event.code !== "ArrowUp" && event.code !== "Home" && event.code !== "End") return;
  const all = [...(dialog?.querySelectorAll<HTMLElement>(selector) ?? [])];
  const at = all.indexOf(event.target as HTMLElement);
  if (at < 0) return;
  event.preventDefault();
  const next =
    event.code === "Home"
      ? 0
      : event.code === "End"
        ? all.length - 1
        : Math.max(0, Math.min(all.length - 1, at + (event.code === "ArrowDown" ? 1 : -1)));
  const target = all[next];
  if (!target) return;
  pick(target);
  target.focus();
}

function onDialogKeydown(event: KeyboardEvent): void {
  if (event.code === "Escape" && !event.defaultPrevented) {
    event.preventDefault();
    close();
    return;
  }
  if (event.code !== "Tab") return;
  // Keep Tab inside the dialog (aria-modal).
  const focusable = [
    ...(dialog?.querySelectorAll<HTMLElement>("button, input, textarea, select, [tabindex]") ?? []),
  ].filter((el) => el.tabIndex >= 0);
  const first = focusable[0];
  const last = focusable[focusable.length - 1];
  if (!first || !last) return;
  if (event.shiftKey && document.activeElement === first) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && document.activeElement === last) {
    event.preventDefault();
    first.focus();
  }
}

async function approve(review: ReviewItem): Promise<void> {
  const message = commit ? commitMessage.trim() : null;
  if (commit && !message) return;
  if ((await inbox.approve(review.id, message)) && inbox.outcome) announcement = outcomeText(inbox.outcome);
}

async function reject(review: ReviewItem): Promise<void> {
  if (await inbox.reject(review.id)) {
    confirmingReject = false;
    if (inbox.outcome) announcement = outcomeText(inbox.outcome);
  }
}

async function undoReject(review: ReviewItem): Promise<void> {
  if ((await inbox.undoReject(review.id)) && inbox.outcome) announcement = outcomeText(inbox.outcome);
}

async function sendComment(review: ReviewItem): Promise<void> {
  const text = comment.trim();
  if (!text) return;
  if (await inbox.comment(review.id, text)) {
    comment = "";
    announcement = "Comment sent. The agent continues, and this review updates when its changes do.";
  }
}

/** Says what actually happened: a reject that restored nothing says so and why. */
function outcomeText(outcome: Outcome): string {
  const left = outcome.conflicts.length === 0 ? "" : ` Left for you: ${outcome.conflicts.join(", ")}.`;
  if (outcome.action === "approved") return `Approved.${left}`;
  if (outcome.action === "undone") {
    const back =
      outcome.restored.length === 0 ? " Nothing needed putting back." : ` Put back: ${outcome.restored.join(", ")}.`;
    return `Reject undone.${back}${left}`;
  }
  if (outcome.restored.length === 0) {
    return `Rejected, but nothing was restored: no file here is known to be the agent's alone, so all of them stay as they are.${left}`;
  }
  return `Rejected. Restored: ${outcome.restored.join(", ")}.${left}`;
}

function onCommentKeydown(event: KeyboardEvent, review: ReviewItem): void {
  if (event.code === "Enter" && (event.ctrlKey || event.metaKey)) {
    event.preventDefault();
    void sendComment(review);
  } else if (event.code === "Escape" && comment) {
    // The first Escape clears the draft, the next one closes the inbox.
    event.preventDefault();
    comment = "";
  }
}
</script>

<button
  type="button"
  class="open-inbox"
  class:asked
  data-testid="inbox-button"
  aria-keyshortcuts={liveKeys(KEYS, inBrowserTab()).replace("Mod", isApple() ? "Meta" : "Control")}
  aria-haspopup="dialog"
  title="Review inbox ({shortcut})"
  onclick={toggle}
>
  Reviews{#if inbox.open.length > 0}{" "}<span class="count">({inbox.open.length} waiting)</span>{/if}
</button>
<span class="visually-hidden" aria-live="polite" data-testid="inbox-asked"
  >{asked ? `An agent asks you to review its changes. Open the review inbox with ${shortcut}.` : ""}</span
>

{#if opened}
  <div class="backdrop">
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class="inbox"
      role="dialog"
      aria-modal="true"
      aria-labelledby="inbox-title"
      tabindex="-1"
      data-testid="inbox"
      bind:this={dialog}
      onkeydown={onDialogKeydown}
    >
      <header>
        <h2 id="inbox-title" tabindex="-1">Review inbox</h2>
        <span class="project">{workspace.project?.name ?? "No project"}</span>
        <button type="button" aria-keyshortcuts="Escape" onclick={close}>Close</button>
      </header>
      {#if inbox.error}
        <p class="error" role="alert">{inbox.error}</p>
      {/if}
      <p class="visually-hidden" role="status">{announcement}</p>

      {#if !workspace.project}
        <p class="empty">Open a project to review its agents' changes.</p>
      {:else if inbox.items.length === 0}
        <p class="empty">{inbox.loading ? "Loading reviews…" : "No agent changes wait for review."}</p>
      {:else}
        <div class="body">
          <nav aria-label="Review items">
            <ul>
              {#each inbox.items as review (review.id)}
                <li>
                  <button
                    type="button"
                    class="entry"
                    data-item={review.id}
                    aria-current={review.id === selectedId}
                    tabindex={review.id === selectedId ? 0 : -1}
                    onclick={() => select(review.id)}
                    onkeydown={(e) =>
                      moveIn(e, "[data-item]", (el) => select(el.dataset.item ?? null))}
                  >
                    <span class="name">{sessionLabel(review)}</span>
                    <span class="state {review.state}">{STATE_LABEL[review.state]}</span>
                    <span class="meta">{itemSummary(review)}</span>
                  </button>
                </li>
              {/each}
            </ul>
          </nav>

          {#if item}
            <section class="detail" aria-labelledby="inbox-item-title" data-testid="inbox-item">
              <div class="item-head">
                <h3 id="inbox-item-title">{sessionLabel(item)}</h3>
                <span class="state {item.state}">{STATE_LABEL[item.state]}</span>
                {#if item.summary}
                  <p class="summary">Agent's summary: {item.summary}</p>
                {/if}
                {#if outcome}
                  <p class="outcome" data-testid="inbox-outcome">
                    {outcomeText(outcome)}
                  </p>
                {/if}
              </div>

              {#if item.state === "open"}
                <div class="actions" role="group" aria-label="Decide on this review">
                  <p class="hint">{approveHint(item)}</p>
                  <label class="check">
                    <input type="checkbox" bind:checked={commit} />
                    Commit the agent's files
                  </label>
                  {#if commit}
                    <label class="field">
                      Commit message
                      <input type="text" bind:value={commitMessage} />
                    </label>
                  {/if}
                  <div class="buttons">
                    <button
                      type="button"
                      class="approve"
                      aria-disabled={busy || (commit && !commitMessage.trim())}
                      onclick={() => {
                        if (!busy) void approve(item);
                      }}>{commit ? "Approve and commit" : "Approve"}</button
                    >
                    {#if !confirmingReject}
                      <button
                        type="button"
                        aria-disabled={busy}
                        onclick={async () => {
                          confirmingReject = true;
                          await tick();
                          dialog?.querySelector<HTMLElement>("[data-confirm-reject]")?.focus();
                        }}>Reject…</button
                      >
                    {/if}
                  </div>
                  {#if confirmingReject}
                    <div class="confirm" role="group" aria-labelledby="inbox-reject-hint">
                      <p id="inbox-reject-hint">{rejectHint(item)}</p>
                      <div class="buttons">
                        <button
                          type="button"
                          class="reject"
                          data-confirm-reject
                          aria-disabled={busy}
                          onclick={() => {
                            if (!busy) void reject(item);
                          }}>Reject the changes</button
                        >
                        <button type="button" onclick={() => (confirmingReject = false)}>Keep reviewing</button>
                      </div>
                    </div>
                  {/if}
                </div>
              {/if}

              {#if item.state === "rejected" && item.can_undo}
                <div class="actions" role="group" aria-label="Undo the reject">
                  <p class="hint">Puts back the files the reject restored. Files changed since stay as they are.</p>
                  <div class="buttons">
                    <button
                      type="button"
                      data-testid="inbox-undo-reject"
                      aria-disabled={busy}
                      onclick={() => {
                        if (!busy) void undoReject(item);
                      }}>Undo reject</button
                    >
                  </div>
                </div>
              {/if}

              {#if item.state !== "approved"}
                <form
                  class="comment"
                  onsubmit={(e) => {
                    e.preventDefault();
                    void sendComment(item);
                  }}
                >
                  <label for="inbox-comment">Comment to the agent</label>
                  <textarea
                    id="inbox-comment"
                    rows="2"
                    bind:value={comment}
                    aria-keyshortcuts="Control+Enter Meta+Enter"
                    onkeydown={(e) => onCommentKeydown(e, item)}
                  ></textarea>
                  <button type="submit" aria-disabled={busy || !comment.trim()}>Send comment ({modEnter})</button>
                </form>
              {/if}

              <div class="files-and-diff">
                <ul class="files" aria-label="Changed files">
                  {#each item.files as changed (changed.path)}
                    {@const markers = fileMarkers(changed)}
                    <li>
                      <button
                        type="button"
                        class="entry"
                        data-file={changed.path}
                        aria-current={changed.path === selectedPath}
                        tabindex={changed.path === selectedPath ? 0 : -1}
                        onclick={() => (selectedPath = changed.path)}
                        onkeydown={(e) =>
                          moveIn(e, "[data-file]", (el) => (selectedPath = el.dataset.file ?? null))}
                      >
                        <span class="name">{changed.path}</span>
                        <span class="meta">{fileSummary(changed)}</span>
                        {#each markers as marker}
                          <span class="marker" data-testid="file-marker">{marker}</span>
                        {/each}
                      </button>
                    </li>
                  {/each}
                </ul>
                <div class="diff" data-testid="inbox-diff">
                  {#if !file}
                    <p class="empty">This review has no files.</p>
                  {:else if diff?.error}
                    <p class="error" role="alert">Could not load the diff of {file.path}: {diff.error}</p>
                  {:else if diff?.patch === null || !diff}
                    <p class="empty">Loading the diff…</p>
                  {:else if diff.patch === ""}
                    <p class="empty">No textual changes in {file.path}.</p>
                  {:else}
                    <PatchView text={diff.patch} />
                  {/if}
                </div>
              </div>
            </section>
          {/if}
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .open-inbox,
  button {
    font: inherit;
    padding: 2px 10px;
    color: #e6e6e6;
    background: #2a2a33;
    border: 1px solid #4a4a57;
    border-radius: 3px;
    cursor: pointer;
  }
  .open-inbox {
    margin-left: auto;
  }
  button[aria-disabled="true"] {
    opacity: 0.55;
    cursor: default;
  }
  button:focus-visible,
  input:focus-visible,
  textarea:focus-visible,
  h2:focus-visible {
    outline: 2px solid #7aa2ff;
    outline-offset: 1px;
  }
  .count {
    color: #ffd36b;
  }
  .open-inbox.asked {
    border-color: #ffd36b;
  }
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 90;
    display: flex;
    padding: 24px;
    background: rgb(0 0 0 / 55%);
  }
  .inbox {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    background: #16161a;
    border: 1px solid #3a3a44;
    border-radius: 6px;
  }
  header {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 12px;
    border-bottom: 1px solid #2a2a31;
  }
  header button {
    margin-left: auto;
  }
  h2 {
    margin: 0;
    font-size: 14px;
  }
  h3 {
    margin: 0;
    font-size: 13px;
  }
  .project,
  .meta,
  .hint,
  .empty,
  .summary {
    color: #b4b4c0;
    font-size: 12px;
  }
  .empty {
    padding: 12px;
    margin: 0;
  }
  .error {
    color: #ff8f8f;
    margin: 6px 12px;
  }
  .body {
    flex: 1;
    display: grid;
    grid-template-columns: 260px 1fr;
    min-height: 0;
  }
  nav {
    overflow: auto;
    border-right: 1px solid #2a2a31;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 4px;
  }
  .entry {
    all: unset;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    gap: 2px;
    width: 100%;
    padding: 6px 8px;
    border-radius: 3px;
    cursor: pointer;
    overflow-wrap: anywhere;
  }
  .entry[aria-current="true"] {
    background: #2a2a38;
    box-shadow: inset 3px 0 0 #7aa2ff;
  }
  .entry:focus-visible {
    outline: 2px solid #7aa2ff;
    outline-offset: -2px;
  }
  .state {
    font-size: 11px;
  }
  .state.open {
    color: #ffd36b;
  }
  .state.approved {
    color: #7fd18b;
  }
  .state.rejected {
    color: #ff8a8a;
  }
  .marker {
    align-self: flex-start;
    padding: 0 4px;
    font-size: 11px;
    color: #ffd36b;
    border: 1px solid currentColor;
    border-radius: 3px;
  }
  .detail {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }
  .item-head,
  .actions,
  .comment {
    padding: 8px 12px;
    border-bottom: 1px solid #2a2a31;
  }
  .item-head {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 8px;
  }
  .item-head p {
    flex-basis: 100%;
    margin: 0;
  }
  .outcome {
    color: #e6e6e6;
  }
  .actions {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .actions p {
    margin: 0;
  }
  .buttons {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .approve {
    border-color: #6bd18a;
  }
  .reject {
    border-color: #ff8a8a;
  }
  .field,
  .comment {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .comment button {
    align-self: flex-start;
  }
  input[type="text"],
  textarea {
    font: inherit;
    color: inherit;
    background: #121216;
    border: 1px solid #4a4a57;
    border-radius: 3px;
  }
  .files-and-diff {
    flex: 1;
    display: grid;
    grid-template-columns: 240px 1fr;
    min-height: 0;
  }
  .files {
    overflow: auto;
    border-right: 1px solid #2a2a31;
  }
  .diff {
    min-width: 0;
    min-height: 0;
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
    .entry[aria-current="true"] {
      outline: 2px solid Highlight;
    }
    .marker {
      border-color: CanvasText;
    }
    button[aria-disabled="true"] {
      color: GrayText;
    }
  }
</style>
