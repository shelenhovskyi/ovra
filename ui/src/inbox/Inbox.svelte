<script lang="ts">
// Reviews, a main-area view: agents' changes waiting for a person. The
// title bar's Reviews button (and Mod+Shift+R, Alt+Shift+R in a browser
// tab) opens it on the project's review items, grouped by task, with the
// chosen one's files and, beside them, that file's diff. Mounted while
// closed too, so it keeps the choice, the drafts and agents' asks.
// Approve, reject and comment are always a person's press; nothing here
// approves or merges on its own. Keys: arrows move in a list, N / P move
// between changes in the diff, Mod+Enter sends a comment, Escape closes
// (after leaving the reject step or the comment field).
import { tick, untrack } from "svelte";
import { agentSessions } from "../agents/sessions.svelte";
import { registerCommand } from "../keys/commands";
import { formatShortcut, isApple } from "../keys/shortcut";
import type { FileChange, ReviewId, ReviewItem, SessionId } from "../lib/protocol";
import { useOvra } from "../shell/context";
import EmptyState from "../shell/EmptyState.svelte";
import Icon from "../shell/Icon.svelte";
import { mainView } from "../shell/main-view.svelte";
import ViewFrame from "../shell/ViewFrame.svelte";
import PatchView from "../viewers/PatchView.svelte";
import {
  approveDetail,
  approveHint,
  defaultCommitMessage,
  fileMarkers,
  fileName,
  fileSummary,
  gitTag,
  gitWarning,
  itemSummary,
  itemTitle,
  keptElsewhere,
  keptFirst,
  notCopiedHint,
  PROTECTED_MARKER,
  protectedHint,
  protectedReason,
  protectedTag,
  rejectHint,
  STATE_LABEL,
  sharedFiles,
  sharedHint,
  shortTitle,
  taskOf,
  workerName,
} from "./describe";
import { REVIEWS_KEYS } from "./keys";
import { handleOpenFor } from "./open";
import { message as errorText, type Outcome, type ReviewInbox } from "./reviews.svelte";

/** The shell's inbox, shared with the title bar's Reviews button. */
let { inbox }: { inbox: ReviewInbox } = $props();
const { transport, workspace } = useOvra();
const sessions = agentSessions(transport);
const modEnter = formatShortcut("Mod+Enter", isApple());

const opened = $derived(mainView.current === "reviews");
let selectedId = $state<ReviewId | null>(null);
let selectedPath = $state<string | null>(null);
let diff = $state<{ key: string; patch: string | null; fingerprint: string | null; error: string | null } | null>(null);
let commit = $state(false);
let commitMessage = $state("");
let confirmingReject = $state(false);
let comment = $state("");
/** Unsent comments of the items not selected now, by item. */
const drafts = new Map<ReviewId, string>();
let announcement = $state("");
/** Approve was stopped because the diff shown was older than the item. */
let staleDiff = $state(false);
/** The items agents asked to show since the inbox was last open. */
let askedFor: ReviewId[] = [];
/** Protected files whose diff was on screen since the inbox opened, by `seenKey`. */
let seen = $state<string[]>([]);
/** The person ticked "Also apply protected files" for a worker's item, at `includedFor`. */
let includeProtected = $state(false);
/** The item's fingerprint when the person ticked: the version they included. */
let includedFor = $state<string | null>(null);
let dialog = $state<HTMLElement>();

const item = $derived(inbox.items.find((i) => i.id === selectedId) ?? null);
const file = $derived(item?.files.find((f) => f.path === selectedPath) ?? null);
const shared = $derived(item ? sharedFiles(item, inbox.items) : new Set<string>());
const sharedText = $derived(item ? sharedHint(item, inbox.items, (o) => sessions.name(o.session)) : null);
/** Files not copied because another worker's version was kept first, with that worker's name. */
const keptFrom = $derived(
  new Map(
    [...(item ? keptElsewhere(item, inbox.items) : [])].map(([path, o]) => [
      path,
      workerName(sessions.name(o.session)),
    ]),
  ),
);
/** Files of the open item whose version another worker's item already copied, with that worker's name. */
const keptBefore = $derived(
  new Map(
    [...(item ? keptFirst(item, inbox.items) : [])].map(([path, o]) => [path, workerName(sessions.name(o.session))]),
  ),
);
const protectedText = $derived(item ? protectedHint(item) : null);
const gitText = $derived(item ? gitWarning(item) : null);
const busy = $derived(item ? inbox.busy.includes(item.id) : false);
/** The list, newest first as the core sends it, a task's workers under the task's name once. */
const groups = $derived.by(() => {
  const out: { task: string | null; items: ReviewItem[] }[] = [];
  for (const review of inbox.items) {
    const task = taskOf(review, sessions.name(review.session));
    const last = out.at(-1);
    if (last && task !== null && last.task === task) last.items.push(review);
    else out.push({ task, items: [review] });
  }
  return out;
});
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
    sharedText ? "inbox-shared" : null,
    gitText ? "inbox-git" : null,
    directWaits ? "inbox-protected-why" : null,
  ]
    .filter(Boolean)
    .join(" ") || undefined,
);

$effect(() => {
  const project = workspace.project?.id ?? null;
  untrack(() => void inbox.show(project));
});

$effect(() =>
  registerCommand({ id: "inbox.toggle", title: "Reviews", keys: REVIEWS_KEYS, keywords: ["inbox"], run: toggle }),
);

// Closed by its frame (the close button, Escape): the reject step is over.
$effect(() => {
  if (!opened) untrack(() => (confirmingReject = false));
});

// A person's Review press for a session or task opens on its waiting item.
$effect(() => handleOpenFor((wanted) => void showFor(wanted)));

// An agent asked to show a review (`open_in_ui`): choose it and say so on the
// button, but never open the view or move focus on an agent's behalf. While
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
        askedFor.push(id);
        inbox.asked = true;
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

// A tick covers the version it was given for: once the files change, or one
// needs a new look, it is gone and the person ticks again.
$effect(() => {
  if (includeProtected && (includableUnseen.length > 0 || (item?.fingerprint ?? null) !== includedFor))
    includeProtected = false;
});

// A protected file counts as seen once its diff, read from the version of
// the files the list shows, is on screen: never while the view is closed,
// where the chosen file's diff still loads.
$effect(() => {
  if (!opened || !item || !file?.protected || !diff || diff.key !== diffKey || diff.patch === null) return;
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
  if (selectedId !== null) drafts.set(selectedId, comment);
  selectedId = id;
  staleDiff = false;
  const next = inbox.items.find((i) => i.id === id);
  // The riskiest file first: its diff is what the person sees.
  selectedPath = (next?.files.find((f) => f.protected) ?? next?.files[0])?.path ?? null;
  includeProtected = false;
  commit = false;
  commitMessage = next ? defaultCommitMessage(next) : "";
  confirmingReject = false;
  comment = (id !== null && drafts.get(id)) || "";
}

/** Opens the inbox; `keep` keeps the item chosen for it. */
async function show(keep = false): Promise<void> {
  mainView.show("reviews");
  // Unless a Review press chose the item: the newest that waits, of those agents asked to show if any.
  const newest = inbox.asked ? inbox.waiting.find((i) => askedFor.includes(i.id)) : inbox.waiting[0];
  if (newest && !keep) select(newest.id);
  inbox.asked = false;
  askedFor = [];

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

/** Opens on the waiting item of the first of `wanted` that has one. */
async function showFor(wanted: SessionId[]): Promise<void> {
  await inbox.reload();
  const found = wanted.map((s) => inbox.waiting.find((i) => i.session === s)).find((i) => i !== undefined);
  if (found) select(found.id);
  if (!opened) await show(true);
}

function close(): void {
  confirmingReject = false;
  mainView.close();
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

/** Escape in the reject step leaves the step; the next one closes the view. */
function onViewKeydown(event: KeyboardEvent): void {
  if (event.code !== "Escape" || event.defaultPrevented || !confirmingReject) return;
  event.preventDefault();
  void cancelReject();
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
    // The note on what was not copied is not read out by itself.
    const now = inbox.items.find((i) => i.id === review.id);
    const left = now?.state === "partly_approved" ? ` ${notCopiedHint(now, keptFrom)}` : "";
    announcement = `${outcomeText(inbox.outcome)}${left}`;
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
  const left =
    outcome.conflicts.length === 0
      ? ""
      : worker && outcome.action === "approved"
        ? // Which and why is said once, in the note below.
          ` ${outcome.conflicts.length === 1 ? "One file was" : "Some files were"} not copied: see why below.`
        : ` Kept as you changed them: ${files}.`;
  if (outcome.action === "resolved") return "Marked done. The files stay as they are now.";
  if (outcome.action === "taken") {
    const put =
      outcome.restored.length > 0 ? `Replaced with the worker's version: ${outcome.restored.join(", ")}.` : "";
    const kept =
      outcome.conflicts.length > 0
        ? ` Not replaced, because the worker changed them after you kept its changes: ${outcome.conflicts.join(", ")}.`
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
    const what = "Kept.";
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
    // The first Escape leaves the field for the item's heading and keeps
    // the draft; the next one closes the view.
    event.preventDefault();
    dialog?.querySelector<HTMLElement>("#inbox-item-title")?.focus();
  }
}
</script>

{#if opened}
  <ViewFrame name="reviews" testid="inbox">
    {#snippet tools()}<span class="project">{workspace.project?.name ?? "No project"}</span>{/snippet}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="inbox" bind:this={dialog} onkeydown={onViewKeydown}>
      {#if inbox.error}
        <p class="error" role="alert">{inbox.error}</p>
      {/if}
      {#if staleDiff}
        <p class="error" role="alert" data-testid="inbox-stale">
          The worker's files changed since this diff loaded, so nothing was kept. Look at them again, then keep the changes.
        </p>
      {/if}
      <p class="visually-hidden" role="status">{announcement}</p>

      {#if !workspace.project}
        <p class="empty">Open a project to review its agents' changes.</p>
      {:else if inbox.items.length === 0}
        {#if inbox.loading}
          <p class="empty">Loading reviews…</p>
        {:else}
          <EmptyState
            centred
            headline="Nothing to review"
            line="Changes from Plan and split wait here until you keep or discard them."
          />
        {/if}
      {:else}
        <div class="body">
          <nav aria-label="Review items">
            <ul>
              {#each groups as group (group.items[0]?.id)}
                {#if group.task}<li class="group" title={group.task}>{group.task}</li>{/if}
                {#each group.items as review (review.id)}
                <li>
                  <button
                    type="button"
                    class="entry"
                    class:flagged={protectedTag(review) !== null || gitTag(review) !== null}
                    data-item={review.id}
                    title={sessionLabel(review)}
                    aria-current={review.id === selectedId}
                    tabindex={review.id === selectedId ? 0 : -1}
                    onclick={() => select(review.id)}
                    onkeydown={(e) =>
                      moveIn(e, "[data-item]", (el) => select(el.dataset.item ?? null))}
                  >
                    <span class="name">{shortTitle(review, sessions.name(review.session))}</span>
                    <span class="meta"
                      ><span class="state {review.state}">{STATE_LABEL[review.state]}</span> · {itemSummary(review)}</span
                    >
                    {#if protectedTag(review)}
                      <span class="marker protected" data-testid="item-protected">{protectedTag(review)}</span>
                    {/if}
                    {#if gitTag(review)}
                      <span class="marker protected" data-testid="item-git">{gitTag(review)}</span>
                    {/if}
                  </button>
                </li>
                {/each}
              {/each}
            </ul>
          </nav>

          {#if item}
            <section class="detail" aria-labelledby="inbox-item-title" data-testid="inbox-item">
              <div class="decide">
              <div class="item-head">
                <h3 id="inbox-item-title" tabindex="-1">{shortTitle(item, sessions.name(item.session))}</h3>
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
                  {#if approveDetail(item) || sharedText}
                    <details class="more">
                      <summary><Icon name="chevron-right" small />Details</summary>
                      {#if approveDetail(item)}<p class="hint">{approveDetail(item)}</p>{/if}
                      {#if sharedText}
                        <p class="hint" id="inbox-shared" data-testid="inbox-shared">{sharedText}</p>
                      {/if}
                    </details>
                  {/if}
                  {#if directWaits}
                    <p class="hint" id="inbox-protected-why" data-testid="inbox-protected-why">
                      Keep changes waits until you have looked at the diff of each of these: {unseen
                        .map(fileName)
                        .join(", ")}. Select each one in the list of changed files.
                    </p>
                  {/if}
                  {#if includable.length > 0}
                    <label class="check">
                      <input
                        type="checkbox"
                        data-testid="inbox-include-protected"
                        checked={includeProtected}
                        onclick={(e) => {
                          // Stays focusable so its explanation is read out, but cannot be ticked yet.
                          if (includableUnseen.length > 0) e.preventDefault();
                        }}
                        onchange={(e) => {
                          if (includableUnseen.length > 0) return;
                          includeProtected = e.currentTarget.checked;
                          includedFor = item.fingerprint;
                        }}
                        aria-disabled={includableUnseen.length > 0}
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
                      <p class="hint" id="inbox-commit-why">Write a commit message to keep and commit.</p>
                    {/if}
                  {/if}
                  {#if !diffShown}
                    <p class="hint" id="inbox-diff-why" data-testid="inbox-diff-why">
                      {diff?.error
                        ? "The diff of this file did not load, so Keep changes waits: you keep only what you have seen."
                        : "Keep changes waits until the diff of this file has loaded."}
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
                      }}>{commit ? "Keep and commit" : "Keep changes"}</button
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
                        <button type="button" data-cancel-reject onclick={cancelReject}>Cancel</button>
                      </div>
                    </div>
                  {/if}
                </div>
              {/if}

              {#if item.state === "partly_approved"}
                <div class="actions" role="group" aria-label="Files that were not copied">
                  <p class="hint" data-testid="inbox-not-copied">{notCopiedHint(item, keptFrom)}</p>
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

                <ul class="files" aria-label="Changed files">
                  {#each item.files as changed (changed.path)}
                    {@const markers = fileMarkers(
                      changed,
                      shared.has(changed.path),
                      item.not_copied.includes(changed.path) ? item.state : null,
                      keptFrom.get(changed.path) ?? null,
                      keptBefore.get(changed.path) ?? null,
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
              </div>
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
            </section>
          {/if}
        </div>
      {/if}
    </div>
  </ViewFrame>
{/if}

<style>
  button {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font: inherit;
    font-weight: 500;
    padding: 5px 12px;
    color: var(--text);
    background: var(--s3);
    border: 0;
    border-radius: var(--radius);
    cursor: pointer;
  }
  button:hover {
    background: var(--s4);
  }
  button[aria-disabled="true"],
  input[aria-disabled="true"] {
    opacity: 0.55;
    cursor: default;
  }

  /* Wide enough: the list, the decision with the files, and the diff beside them. */
  .inbox {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-height: 0;
    container-type: inline-size;
  }
  h3 {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
  }
  .project,
  .meta,
  .hint,
  .empty,
  .summary,
  .field {
    color: var(--text-2);
    font-size: 13px;
  }
  .empty {
    padding: 12px 20px;
    margin: 0;
  }
  .error {
    color: var(--danger);
    margin: 4px 20px;
  }
  .body {
    flex: 1;
    display: grid;
    grid-template-columns: 220px minmax(0, 1fr);
    min-height: 0;
  }
  nav {
    overflow: auto;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 4px 8px;
  }
  .entry {
    all: unset;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    gap: 2px;
    width: 100%;
    padding: 8px 10px;
    border-radius: var(--radius);
    cursor: pointer;
    overflow-wrap: anywhere;
  }
  .entry:hover,
  .entry[aria-current="true"] {
    background: var(--s3);
  }
  .entry[aria-current="true"] .name {
    font-weight: 500;
  }
  .entry:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
  /* Items with protected files or git changes stand out in the list. */
  /* One line each: the full title is the entry's tooltip. */
  .entry .name,
  .entry .meta,
  .group {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .group {
    padding: 10px 10px 2px;
    font-size: 13px;
    font-weight: 600;
    color: var(--text-2);
  }
  .more summary {
    display: flex;
    align-items: center;
    gap: 4px;
    width: fit-content;
    font-size: 13px;
    color: var(--text-2);
    list-style: none;
    cursor: pointer;
  }
  .more summary::-webkit-details-marker {
    display: none;
  }
  .more[open] summary :global(svg) {
    transform: rotate(90deg);
  }
  .more p {
    margin: 6px 0 0;
  }
  .entry.flagged .name::before {

    content: "";
    display: inline-block;
    width: 7px;
    height: 7px;
    margin-right: 8px;
    border-radius: 50%;
    background: var(--danger);
    vertical-align: middle;
  }
  .state {
    font-size: 13px;
    font-weight: 500;
  }
  .state.open,
  .state.partly_approved {
    color: var(--warn);
  }
  .state.approved {
    color: var(--ok);
  }
  .state.rejected {
    color: var(--danger);
  }
  .marker {
    align-self: flex-start;
    padding: 1px 7px;
    font-size: 12px;
    font-weight: 500;
    color: var(--warn);
    background: var(--warn-soft);
    border-radius: var(--radius-sm);
  }
  .detail {
    display: grid;
    grid-template-columns: minmax(280px, 360px) minmax(0, 1fr);
    min-width: 0;
    min-height: 0;
  }
  .decide {
    display: flex;
    flex-direction: column;
    min-height: 0;
    padding-bottom: 12px;
    overflow: auto;
  }
  .item-head,
  .actions,
  .comment {
    padding: 12px 20px 0;
  }
  .item-head {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 4px 10px;
    padding-top: 16px;
  }
  .item-head p {
    flex-basis: 100%;
    margin: 0;
  }
  .outcome {
    color: var(--text);
  }
  .actions {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .actions p {
    margin: 0;
  }
  .buttons {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .approve,
  .approve:hover {
    color: var(--accent-ink);
    background: var(--accent);
  }
  .approve:hover {
    filter: brightness(1.08);
  }
  .reject {
    color: var(--danger);
  }
  .check {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  input[type="checkbox"] {
    accent-color: var(--accent);
  }
  .field,
  .comment {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .comment label {
    color: var(--text-2);
    font-size: 13px;
  }
  .comment button {
    align-self: flex-start;
  }
  input[type="text"],
  textarea {
    font: inherit;
    color: var(--text);
    padding: 6px 10px;
    background: var(--s3);
    border: 0;
    border-radius: var(--radius);
  }
  .files {
    margin-top: 12px;
  }
  .diff {
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    background: var(--s1);
    border-top-left-radius: 10px;
  }
  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
  /* A narrow main area: the diff goes under the decision, which keeps the width. */
  @container (max-width: 860px) {
    .detail {
      display: flex;
      flex-direction: column;
      overflow: auto;
    }
    .decide {
      flex: none;
      overflow: visible;
    }
    .diff {
      flex: none;
      height: 60vh;
    }
  }
  @container (max-width: 560px) {
    .body {
      grid-template-columns: minmax(0, 1fr);
      grid-template-rows: auto minmax(0, 1fr);
    }
    nav {
      max-height: 25vh;
    }
  }
  .marker.protected,
  p.protected {
    color: var(--danger);
    font-weight: 500;
  }
  .marker.protected,
  #inbox-git {
    background: color-mix(in srgb, var(--danger) 13%, transparent);
  }
  p.protected {
    margin: 0;
    font-size: 13px;
  }
  /* Git hooks and config: nothing else on the item shows them. */
  #inbox-git {
    padding: 8px 12px;
    border-radius: var(--radius);
  }
  @media (forced-colors: active) {
    .entry[aria-current="true"] {
      outline: 2px solid Highlight;
    }
    .marker,
    #inbox-git {
      border: 1px solid CanvasText;
    }
    button[aria-disabled="true"],
    input[aria-disabled="true"] {
      color: GrayText;
    }
  }
</style>
