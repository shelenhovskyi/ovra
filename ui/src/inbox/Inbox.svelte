<script lang="ts">
// The review inbox: agents' changes waiting for a person. A header button
// (and Mod+Shift+R, Alt+Shift+R in a browser tab) opens a dialog with the
// project's review items, the files of the chosen one and that file's diff.
// Approve, reject and comment are always a person's press; nothing here
// approves or merges on its own. Keys: arrows move in a list, N / P move
// between changes in the diff, Mod+Enter sends a comment, Escape closes.
import { tick, untrack } from "svelte";
import { agentSessions } from "../agents/sessions.svelte";
import { registerCommand } from "../keys/commands";
import { formatShortcut, inBrowserTab, isApple, liveKeys, matchesShortcut, parseShortcut } from "../keys/shortcut";
import type { FileChange, ReviewId, ReviewItem } from "../lib/protocol";
import { useOvra } from "../shell/context";
import PatchView from "../viewers/PatchView.svelte";
import {
  approveHint,
  defaultCommitMessage,
  fileMarkers,
  fileName,
  fileSummary,
  gitTag,
  gitWarning,
  itemSummary,
  itemTitle,
  notCopiedHint,
  PROTECTED_MARKER,
  protectedHint,
  protectedReason,
  protectedTag,
  rejectHint,
  STATE_LABEL,
  sharedFiles,
} from "./describe";
import { message as errorText, type Outcome, ReviewInbox } from "./reviews.svelte";

const KEYS = "Mod+Shift+R";
const { transport, workspace } = useOvra();
const inbox = new ReviewInbox(transport);
const sessions = agentSessions(transport);
const shortcut = formatShortcut(liveKeys(KEYS, inBrowserTab()), isApple());
const modEnter = formatShortcut("Mod+Enter", isApple());

let opened = $state(false);
let selectedId = $state<ReviewId | null>(null);
let selectedPath = $state<string | null>(null);
let diff = $state<{ key: string; patch: string | null; fingerprint: string | null; error: string | null } | null>(null);
let commit = $state(false);
let commitMessage = $state("");
let confirmingReject = $state(false);
let comment = $state("");
let announcement = $state("");
/** Approve was stopped because the diff shown was older than the item. */
let staleDiff = $state(false);
/** An agent asked for a review since the inbox was last open. */
let asked = $state(false);
/** Protected files whose diff was on screen since the inbox opened, by `seenKey`. */
let seen = $state<string[]>([]);
/** The person ticked "Also apply protected files" for a worker's item. */
let includeProtected = $state(false);
let dialog = $state<HTMLElement>();
let returnTo: HTMLElement | null = null;

const item = $derived(inbox.items.find((i) => i.id === selectedId) ?? null);
const file = $derived(item?.files.find((f) => f.path === selectedPath) ?? null);
const shared = $derived(item ? sharedFiles(item, inbox.items) : new Set<string>());
const protectedText = $derived(item ? protectedHint(item) : null);
const gitText = $derived(item ? gitWarning(item) : null);
const busy = $derived(item ? inbox.busy.includes(item.id) : false);
const outcome = $derived(inbox.outcome && inbox.outcome.id === item?.id ? inbox.outcome : null);
const diffKey = $derived(item && file ? `${item.id}\n${file.path}\n${inbox.changes}` : null);
/**
 * A worker's item, and a direct one with protected files, is approved only
 * with the current file's diff on screen, read from the same version of its
 * files as the list (`fingerprint`).
 */
const diffShown = $derived(
  !item?.fingerprint || (diff !== null && diff.key === diffKey && diff.patch !== null && diff.fingerprint !== null),
);
/** Protected files of the item whose diff the person has not seen in this version. */
const unseen = $derived(item ? item.files.filter((f) => f.protected && !seen.includes(seenKey(item, f))) : []);
/** A worker's protected files that approve could copy: those not waiting for the direct agent. */
const includable = $derived(item?.mode === "isolated" ? item.files.filter((f) => f.protected && !f.blocked) : []);
const includableUnseen = $derived(includable.filter((f) => unseen.includes(f)));
/** A direct item's protected files are already on disk: Approve keeps them only once each was seen. */
const directWaits = $derived(item?.mode === "direct" && unseen.length > 0);
/** The protected files a replace could copy: whatever is left, including those that waited at approve. */
const takeGuarded = $derived(
  item?.state === "partly_approved"
    ? item.files.filter((f) => f.protected && [f.path, f.from].some((p) => p && item.not_copied.includes(p)))
    : [],
);
/** Before a replace, each protected file it would copy must have been seen too. */
const takeUnseen = $derived(takeGuarded.filter((f) => unseen.includes(f)));
/** What Approve's description points at: why it waits, and the protected paths it would apply. */
const approveWhy = $derived(
  [
    commit && !commitMessage.trim() ? "inbox-commit-why" : diffShown ? null : "inbox-diff-why",
    protectedText ? "inbox-protected" : null,
    gitText ? "inbox-git" : null,
    directWaits ? "inbox-protected-why" : null,
  ]
    .filter(Boolean)
    .join(" ") || undefined,
);

$effect(() => () => inbox.dispose());

$effect(() => {
  const project = workspace.project?.id ?? null;
  untrack(() => void inbox.show(project));
});

$effect(() => registerCommand({ id: "inbox.toggle", title: "Review inbox", keys: KEYS, run: toggle }));

// An agent asked to show a review (`open_in_ui`): choose it and say so on the
// button, but never open the dialog or move focus on an agent's behalf. While
// the person reviews, the choice stays theirs: an agent switching the item
// under a focused Approve could get the wrong work approved.
$effect(() =>
  transport.onEvent((event) => {
    if (event.type !== "orchestration.show" || event.data.target.kind !== "review") return;
    if (event.data.project !== workspace.project?.id) return;
    const id = event.data.target.id;
    void inbox.reload().then(() => {
      if (!opened) {
        select(id);
        asked = true;
      } else if (id !== selectedId) {
        const wanted = inbox.items.find((i) => i.id === id);
        if (wanted) announcement = `An agent asks you to look at ${sessionLabel(wanted)}. It is in the list.`;
      }
    });
  }),
);

// Keep a valid item and file selected as the list changes.
$effect(() => {
  const items = inbox.items;
  untrack(() => {
    if (!items.some((i) => i.id === selectedId)) select((inbox.waiting[0] ?? inbox.open[0] ?? items[0])?.id ?? null);
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
    if (!diff?.key.startsWith(`${id}\n${path}\n`)) diff = { key, patch: null, fingerprint: null, error: null };
  });
  inbox.diff(id, path).then(
    (loaded) => {
      if (live) diff = { key, ...loaded, error: null };
    },
    (err: unknown) => {
      if (live) diff = { key, patch: null, fingerprint: null, error: errorText(err) };
    },
  );
  return () => {
    live = false;
  };
});

// A protected file counts as seen once its diff, read from the version of
// the files the list shows, is on screen.
$effect(() => {
  if (!item || !file?.protected || !diff || diff.key !== diffKey || diff.patch === null) return;
  if (item.fingerprint && diff.fingerprint !== item.fingerprint) return;
  const key = seenKey(item, file);
  untrack(() => {
    if (!seen.includes(key)) seen = [...seen, key];
  });
});

/**
 * Which version of a file was seen: by the item's fingerprint (a worker's
 * files, a direct item's protected files) and the file's change, so a
 * later change needs a new look.
 */
function seenKey(review: ReviewItem, f: FileChange): string {
  return [review.id, review.fingerprint ?? "", f.path, f.from ?? "", f.kind, f.additions, f.deletions].join("\n");
}

function select(id: ReviewId | null): void {
  if (id === selectedId) return;
  selectedId = id;
  staleDiff = false;
  const next = inbox.items.find((i) => i.id === id);
  // The riskiest file first: its diff is what the person sees.
  selectedPath = (next?.files.find((f) => f.protected) ?? next?.files[0])?.path ?? null;
  includeProtected = false;
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
  // Seen means seen in this sitting, not some earlier one.
  seen = [];
  includeProtected = false;
  // Never show a list older than the moment the person looks at it.
  void inbox.reload();
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
  return itemTitle(review, sessions.name(review.session));
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

async function cancelReject(): Promise<void> {
  confirmingReject = false;
  await tick();
  dialog?.querySelector<HTMLElement>("[data-start-reject]")?.focus();
}

const toggleKeys = parseShortcut(liveKeys(KEYS, inBrowserTab()), isApple());

function onDialogKeydown(event: KeyboardEvent): void {
  // Shortcuts of the panels behind the modal must not act; only the inbox's own toggle gets through.
  if (!(toggleKeys && matchesShortcut(event, toggleKeys))) event.stopPropagation();
  if (event.code === "Escape" && !event.defaultPrevented) {
    event.preventDefault();
    // Escape in the reject step leaves the step, not the inbox.
    if (confirmingReject) void cancelReject();
    else close();
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

/**
 * After an action the pressed button is gone: focus the undo button when there
 * is one, else the item's heading, so keyboard users stay on this item.
 */
async function refocus(): Promise<void> {
  await tick();
  const target =
    dialog?.querySelector<HTMLElement>("[data-testid=inbox-undo-reject]") ??
    dialog?.querySelector<HTMLElement>("#inbox-item-title");
  target?.focus();
}

async function approve(review: ReviewItem): Promise<void> {
  const message = commit ? commitMessage.trim() : null;
  if (commit && !message) return;
  // The diff on screen must be of the files approve would land: if it was
  // read from another version of the worker's files, show both anew first.
  if (review.fingerprint && !diffShown) return;
  if (directWaits) return;
  const shown = diff?.fingerprint ?? null;
  if (review.fingerprint && shown !== review.fingerprint) {
    staleDiff = true;
    await inbox.recheck();
    return;
  }
  staleDiff = false;
  // A direct item's protected files are kept once each was seen; a worker's
  // are copied only if the person ticked the box after seeing each.
  const included =
    review.mode === "direct"
      ? review.files.some((f) => f.protected)
      : includeProtected && includable.length > 0 && includableUnseen.length === 0;
  if ((await inbox.approve(review.id, message, included)) && inbox.outcome) {
    announcement = outcomeText(inbox.outcome);
    await refocus();
  }
}

async function reject(review: ReviewItem): Promise<void> {
  if (await inbox.reject(review.id)) {
    confirmingReject = false;
    if (inbox.outcome) announcement = outcomeText(inbox.outcome);
    await refocus();
  }
}

async function undoReject(review: ReviewItem): Promise<void> {
  if ((await inbox.undoReject(review.id)) && inbox.outcome) {
    announcement = outcomeText(inbox.outcome);
    await refocus();
  }
}

async function take(review: ReviewItem): Promise<void> {
  if (takeUnseen.length > 0) return;
  // Each protected file left was seen: replacing copies them too.
  if ((await inbox.take(review.id, takeGuarded.length > 0)) && inbox.outcome) {
    announcement = outcomeText(inbox.outcome);
    await refocus();
  }
}

async function undoTake(review: ReviewItem): Promise<void> {
  if ((await inbox.undoTake(review.id)) && inbox.outcome) {
    announcement = outcomeText(inbox.outcome);
    await refocus();
  }
}

async function resolve(review: ReviewItem): Promise<void> {
  if ((await inbox.resolve(review.id)) && inbox.outcome) {
    announcement = outcomeText(inbox.outcome);
    await refocus();
  }
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
  const of = inbox.items.find((i) => i.id === outcome.id);
  const worker = of?.mode === "isolated";
  const files = outcome.conflicts.join(", ");
  // A worker's protected files that approve left out, named as such.
  const guarded = (path: string) =>
    of?.files.some((f) => f.protected && !f.blocked && (f.path === path || f.from === path)) ?? false;
  const leftOut = outcome.conflicts.filter(guarded);
  const others = outcome.conflicts.filter((p) => !guarded(p));
  const left =
    outcome.conflicts.length === 0
      ? ""
      : worker && outcome.action === "approved"
        ? [
            others.length > 0 ? ` Not copied: ${others.join(", ")}.` : "",
            leftOut.length > 0
              ? ` Not copied, as files that can run code or change agent rules: ${leftOut.join(", ")}.`
              : "",
          ].join("")
        : ` Kept as you changed them: ${files}.`;
  if (outcome.action === "resolved") return "Marked done. The files stay as they are now.";
  if (outcome.action === "taken") {
    const put =
      outcome.restored.length > 0 ? `Replaced with the worker's version: ${outcome.restored.join(", ")}.` : "";
    const kept =
      outcome.conflicts.length > 0
        ? ` Not replaced, because the worker changed them after you approved: ${outcome.conflicts.join(", ")}.`
        : "";
    return `${put}${kept}`.trim() || "Nothing was replaced.";
  }
  if (outcome.action === "untaken") {
    const back = outcome.restored.length > 0 ? `Your versions are back: ${outcome.restored.join(", ")}.` : "";
    const left =
      outcome.conflicts.length > 0
        ? ` Not put back, because you changed them after the replace: ${outcome.conflicts.join(", ")}.`
        : "";
    return `${back}${left}`.trim() || "Nothing needed putting back.";
  }
  if (outcome.action === "approved") {
    const what = outcome.conflicts.length > 0 && worker ? "Approved, not all copied." : "Approved.";
    const committed = outcome.commit ? ` Committed as “${outcome.commit}”.` : "";
    return `${what}${committed}${left}`;
  }
  if (outcome.action === "undone") {
    const back =
      outcome.restored.length === 0 ? " Nothing needed putting back." : ` Put back: ${outcome.restored.join(", ")}.`;
    return `Reject undone.${back}${left}`;
  }
  if (outcome.restored.length === 0) {
    return `Rejected, but nothing was restored: no file here was changed only by the agent's file tools, so all of them stay as they are.${left}`;
  }
  if (worker) return `Rejected. Discarded from the worker's copy: ${outcome.restored.join(", ")}.${left}`;
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
  Reviews{#if inbox.waiting.length > 0}{" "}<span class="count">({inbox.waiting.length} waiting)</span>{/if}
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
      {#if staleDiff}
        <p class="error" role="alert" data-testid="inbox-stale">
          The worker's files changed since this diff loaded, so nothing was approved. Look at them again, then approve.
        </p>
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
                    class:flagged={protectedTag(review) !== null || gitTag(review) !== null}
                    data-item={review.id}
                    aria-current={review.id === selectedId}
                    tabindex={review.id === selectedId ? 0 : -1}
                    onclick={() => select(review.id)}
                    onkeydown={(e) =>
                      moveIn(e, "[data-item]", (el) => select(el.dataset.item ?? null))}
                  >
                    <span class="name">{sessionLabel(review)}</span>
                    <span class="state {review.state}">{STATE_LABEL[review.state]}</span>
                    <span class="meta">{itemSummary(review, sharedFiles(review, inbox.items).size)}</span>
                    {#if protectedTag(review)}
                      <span class="marker protected" data-testid="item-protected">{protectedTag(review)}</span>
                    {/if}
                    {#if gitTag(review)}
                      <span class="marker protected" data-testid="item-git">{gitTag(review)}</span>
                    {/if}
                  </button>
                </li>
              {/each}
            </ul>
          </nav>

          {#if item}
            <section class="detail" aria-labelledby="inbox-item-title" data-testid="inbox-item">
              <div class="item-head">
                <h3 id="inbox-item-title" tabindex="-1">{sessionLabel(item)}</h3>
                <span class="state {item.state}">{STATE_LABEL[item.state]}</span>
                {#if item.summary}
                  <p class="summary">Agent's summary: {item.summary}</p>
                {/if}
                {#if gitText}
                  <p class="protected" id="inbox-git" role="note" data-testid="inbox-git">{gitText}</p>
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
                  {#if protectedText}
                    <p class="protected" id="inbox-protected" data-testid="inbox-protected">{protectedText}</p>
                  {/if}
                  {#if directWaits}
                    <p class="hint" id="inbox-protected-why" data-testid="inbox-protected-why">
                      Approve waits until you have looked at the diff of each of these: {unseen
                        .map(fileName)
                        .join(", ")}. Select each one in the list of changed files.
                    </p>
                  {/if}
                  {#if includable.length > 0}
                    <label class="check">
                      <input
                        type="checkbox"
                        data-testid="inbox-include-protected"
                        checked={includeProtected && includableUnseen.length === 0}
                        onchange={(e) => (includeProtected = e.currentTarget.checked)}
                        disabled={includableUnseen.length > 0}
                        aria-describedby={includableUnseen.length > 0 ? "inbox-include-why" : undefined}
                      />
                      Also apply protected files: {includable.map(fileName).join(", ")}
                    </label>
                    {#if includableUnseen.length > 0}
                      <p class="hint" id="inbox-include-why" data-testid="inbox-include-why">
                        To include them, look at the diff of each first: {includableUnseen.map(fileName).join(", ")}.
                      </p>
                    {/if}
                  {/if}
                  <label class="check">
                    <input type="checkbox" bind:checked={commit} />
                    Commit the agent's files
                  </label>
                  {#if commit}
                    <label class="field">
                      Commit message
                      <input type="text" bind:value={commitMessage} aria-describedby="inbox-commit-why" />
                    </label>
                    {#if !commitMessage.trim()}
                      <p class="hint" id="inbox-commit-why">Write a commit message to approve and commit.</p>
                    {/if}
                  {/if}
                  {#if !diffShown}
                    <p class="hint" id="inbox-diff-why" data-testid="inbox-diff-why">
                      {diff?.error
                        ? "The diff of this file did not load, so approve waits: you approve only what you have seen."
                        : "Approve waits until the diff of this file has loaded."}
                    </p>
                  {/if}
                  <div class="buttons">
                    <button
                      type="button"
                      class="approve"
                      aria-disabled={busy || (commit && !commitMessage.trim()) || !diffShown || directWaits}
                      aria-describedby={approveWhy}
                      onclick={() => {
                        if (!busy) void approve(item);
                      }}>{commit ? "Approve and commit" : "Approve"}</button
                    >
                    {#if !confirmingReject}
                      <button
                        type="button"
                        data-start-reject
                        aria-disabled={busy}
                        onclick={async () => {
                          confirmingReject = true;
                          await tick();
                          // Start on the safe choice, so a second Enter does not reject.
                          dialog?.querySelector<HTMLElement>("[data-cancel-reject]")?.focus();
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
                        <button type="button" data-cancel-reject onclick={cancelReject}>Keep reviewing</button>
                      </div>
                    </div>
                  {/if}
                </div>
              {/if}

              {#if item.state === "partly_approved"}
                <div class="actions" role="group" aria-label="Files that were not copied">
                  <p class="hint" data-testid="inbox-not-copied">{notCopiedHint(item)}</p>
                  {#if takeUnseen.length > 0}
                    <p class="hint" id="inbox-take-why" data-testid="inbox-take-why">
                      Replacing waits until you have looked at the diff of each file that can run code or change agent
                      rules: {takeUnseen.map(fileName).join(", ")}.
                    </p>
                  {/if}
                  <div class="buttons">
                    <button
                      type="button"
                      data-testid="inbox-take"
                      aria-disabled={busy || takeUnseen.length > 0}
                      aria-describedby={takeUnseen.length > 0 ? "inbox-take-why" : undefined}
                      onclick={() => {
                        if (!busy) void take(item);
                      }}>Replace mine with the worker's version</button
                    >
                    <button
                      type="button"
                      data-testid="inbox-resolve"
                      aria-disabled={busy}
                      onclick={() => {
                        if (!busy) void resolve(item);
                      }}>Mark done</button
                    >
                  </div>
                </div>
              {/if}

              {#if item.can_undo_take}
                <div class="actions" role="group" aria-label="Undo the copy">
                  <p class="hint">Puts back your versions from just before you replaced them.</p>
                  <div class="buttons">
                    <button
                      type="button"
                      data-testid="inbox-undo-take"
                      aria-disabled={busy}
                      onclick={() => {
                        if (!busy) void undoTake(item);
                      }}>Undo replace</button
                    >
                    {#if item.state === "approved"}
                      <button
                        type="button"
                        data-testid="inbox-keep-take"
                        aria-disabled={busy}
                        onclick={() => {
                          if (!busy) void resolve(item);
                        }}>Keep the worker's version</button
                      >
                    {/if}
                  </div>
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
                    {@const markers = fileMarkers(
                      changed,
                      shared.has(changed.path),
                      item.not_copied.includes(changed.path) ? item.state : null,
                    )}
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
                          <span
                            class="marker"
                            class:protected={marker === PROTECTED_MARKER}
                            title={marker === PROTECTED_MARKER ? protectedReason(changed) : undefined}
                            data-testid="file-marker">{marker}</span
                          >
                        {/each}
                        {#if changed.protected}
                          <span class="meta" data-testid="file-reason">{protectedReason(changed)}</span>
                        {/if}
                      </button>
                    </li>
                  {/each}
                </ul>
                <div class="diff" data-testid="inbox-diff">
                  {#if !file}
                    <p class="empty">
                      {item.git_changes.length > 0
                        ? "No file of the project changed, so there is no diff. Only git hooks or config changed: see the warning above."
                        : "This review has no files."}
                    </p>
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
  h2:focus-visible,
  h3:focus-visible {
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
  /* Items with protected files or git changes stand out in the list. */
  .entry.flagged {
    border-left: 3px solid #ff8f8f;
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
  .state.partly_approved {
    color: #ffd36b;
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
  /* Narrow windows and 200% zoom: stack the lists over the diff so it keeps the width. */
  @media (max-width: 900px) {
    .backdrop {
      padding: 0;
    }
    .body,
    .files-and-diff {
      grid-template-columns: 1fr;
      grid-template-rows: auto 1fr;
    }
    nav,
    .files {
      max-height: 25vh;
      border-right: none;
      border-bottom: 1px solid #2a2a31;
    }
    .detail {
      overflow: auto;
    }
    .diff {
      min-height: 60vh;
    }
  }
  .marker.protected,
  p.protected {
    color: #ff8f8f;
    font-weight: 600;
  }
  p.protected {
    margin: 0;
    font-size: 12px;
  }
  /* Git hooks and config: nothing else on the item shows them. */
  #inbox-git {
    padding: 6px 8px;
    border: 1px solid #ff8f8f;
    border-radius: 3px;
  }
  @media (forced-colors: active) {
    .entry[aria-current="true"] {
      outline: 2px solid Highlight;
    }
    .marker,
    #inbox-git {
      border-color: CanvasText;
    }
    button[aria-disabled="true"] {
      color: GrayText;
    }
  }
</style>
