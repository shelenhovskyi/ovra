<script lang="ts">
// Pending permission requests, oldest first. Keyboard: Alt+Shift+A (Mod+Shift+A
// in the desktop app) jumps here; on a focused card Y allows, N denies,
// Shift+N denies with a message, arrows move between cards, Escape goes back.
//
// Nothing is answered by accident:
// - focus never moves here from elsewhere;
// - a request is answerable only ARM_MS after it appeared, after the previous
//   answer and after any card moved (a request left the list or the list
//   was reloaded in another order), and the
//   buttons look disabled exactly as long as they are;
// - one key press answers at most one request: auto-repeat is ignored and the
//   next answer needs the key released first.
import { tick, untrack } from "svelte";
import { SvelteMap } from "svelte/reactivity";
import { agentSessions, sessionName } from "../agents/sessions.svelte";
import { registerCommand } from "../keys/commands";
import type { Decision, PermissionId, PermissionRequest, Project, ProjectId } from "../lib/protocol";
import { useOvra } from "../shell/context";
import { describeTool } from "./describe";
import { ARM_MS, approvalQueue } from "./queue.svelte";
import { pathScopes, sameFolder } from "./scope";

const { transport, workspace } = useOvra();
const queue = approvalQueue(transport);
const sessions = agentSessions(transport);

let now = $state(Date.now());
/** The last answer or the last time a request left the list. */
let quietFrom = $state(0);
/** A key answered and has not been released yet. */
let keyHeld = $state(false);
let denying = $state<string | null>(null);
let note = $state("");
let list = $state<HTMLElement>();
let heading = $state<HTMLElement>();
let returnTo: HTMLElement | null = null;
/** Said to screen readers: what a jump to a request found. */
let notice = $state("");
const projects = new SvelteMap<ProjectId, Project>();
/** Projects already asked for, so a project the core does not know is asked once. */
const requested = new Set<ProjectId>();

const armedAt = (id: string): number => Math.max(queue.arrived.get(id) ?? 0, quietFrom) + ARM_MS;
/** The one test for both the look of the buttons and whether a press counts. */
const armed = (id: string): boolean => !keyHeld && now >= armedAt(id) && !queue.answering.has(id);

function quiet(): void {
  quietFrom = Date.now();
  now = quietFrom;
}

// Re-render when the next request becomes answerable.
$effect(() => {
  const waits = queue.pending.map((r) => armedAt(r.id)).filter((t) => t > now);
  if (waits.length === 0) return;
  const timer = setTimeout(() => (now = Date.now()), Math.max(0, Math.min(...waits) - Date.now()));
  return () => clearTimeout(timer);
});

// A card that moves (or leaves) puts another one under the pointer.
let shown: PermissionId[] = [];
$effect(() => {
  const ids = queue.pending.map((r) => r.id);
  untrack(() => {
    if (shown.some((id, i) => ids[i] !== id)) quiet();
  });
  shown = ids;
});

$effect(() => {
  const release = () => {
    // The next card arms ARM_MS after the key that answered comes up.
    if (keyHeld) quiet();
    keyHeld = false;
  };
  window.addEventListener("keyup", release);
  // A key released in another window sends no keyup here; held keys only repeat, which is ignored.
  window.addEventListener("blur", release);
  return () => {
    window.removeEventListener("keyup", release);
    window.removeEventListener("blur", release);
  };
});

// Project names and folders for the cards.
$effect(() => {
  const missing = queue.pending.filter((r) => !projects.has(r.project) && !requested.has(r.project));
  if (missing.length === 0) return;
  for (const r of missing) requested.add(r.project);
  untrack(() => {
    transport
      .call({ method: "projects.list" })
      .then(({ data }) => {
        for (const project of data) projects.set(project.id, project);
      })
      .catch(() => {});
  });
});

$effect(() =>
  registerCommand({ id: "approvals.focus", title: "Go to pending approvals", keys: "Mod+Shift+A", run: enter }),
);

$effect(() => {
  queue.revealer = (id) => {
    remember();
    const target = card(id);
    // Never another request instead: a key press meant for this one would answer it.
    notice = target ? "" : "That request was already answered.";
    (target ?? heading)?.focus();
  };
  return () => {
    queue.revealer = null;
  };
});

function cards(): HTMLElement[] {
  return [...(list?.querySelectorAll<HTMLElement>("[data-request]") ?? [])];
}

function card(id: PermissionId): HTMLElement | undefined {
  return cards().find((c) => c.dataset.request === id);
}

function remember(): void {
  const active = document.activeElement;
  if (active instanceof HTMLElement && !list?.contains(active) && active !== heading) returnTo = active;
}

function enter(): void {
  remember();
  (cards()[0] ?? heading)?.focus();
}

function leave(): void {
  if (returnTo?.isConnected) returnTo.focus();
  returnTo = null;
}

async function answer(request: PermissionRequest, decision: Decision, message?: string): Promise<void> {
  if (!armed(request.id)) return;
  // Only a request already on screen may take the focus next: one that
  // arrives while the answer is on its way was never seen.
  const before = queue.pending.map((r) => r.id);
  const index = before.indexOf(request.id);
  notice = "";
  const focused = list?.contains(document.activeElement) ?? false;
  quiet();
  const sent = await queue.answer(request.id, decision, message);
  if (!sent) return;
  if (denying === request.id) {
    denying = null;
    note = "";
  }
  await tick();
  if (!focused) return;
  const seen = [...before.slice(index + 1), ...before.slice(0, index).reverse()];
  const next = seen.map(card).find((c) => c !== undefined);
  // With nothing seen left, stay on the heading. A request that arrives later
  // is never focused on its own: typing must not land on it.
  (next ?? heading)?.focus();
}

/** Answers from a key press: at most one answer per press. */
function answerByKey(event: KeyboardEvent, request: PermissionRequest, decision: Decision, message?: string): void {
  if (event.repeat || keyHeld || !armed(request.id)) return;
  // `answer` checks `armed` before its first await, so mark the key after.
  void answer(request, decision, message);
  keyHeld = true;
}

async function openDeny(request: PermissionRequest): Promise<void> {
  denying = request.id;
  note = "";
  await tick();
  card(request.id)?.querySelector<HTMLTextAreaElement>("textarea")?.focus();
}

function onKeydown(event: KeyboardEvent, request: PermissionRequest): void {
  if (event.target instanceof HTMLTextAreaElement) {
    if (event.code === "Escape") {
      event.preventDefault();
      denying = null;
      card(request.id)?.focus();
    } else if (event.code === "Enter" && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      answerByKey(event, request, "deny", note.trim() || undefined);
    }
    return;
  }
  // Buttons answer by click (Enter, Space); everything else is the card's.
  if (event.target instanceof HTMLButtonElement && (event.code === "Enter" || event.code === "Space")) return;
  if (event.ctrlKey || event.metaKey || event.altKey || event.isComposing) return;
  const all = cards();
  const at = all.findIndex((c) => c.dataset.request === request.id);
  // Y and N answer only on the card itself, never from a control inside it.
  const onCard = event.target === event.currentTarget;
  const handled = (() => {
    switch (event.code) {
      case "KeyY":
        if (event.shiftKey || !onCard) return false;
        answerByKey(event, request, "allow");
        return true;
      case "KeyN":
        if (!onCard) return false;
        if (event.shiftKey) {
          if (!event.repeat) void openDeny(request);
        } else answerByKey(event, request, "deny");
        return true;
      case "ArrowDown":
        all[Math.min(at + 1, all.length - 1)]?.focus();
        return true;
      case "ArrowUp":
        all[Math.max(at - 1, 0)]?.focus();
        return true;
      case "Escape":
        leave();
        return true;
      default:
        return false;
    }
  })();
  if (handled) event.preventDefault();
}

/** Whether the agent's title only repeats what the card already shows. */
function repeats(title: string, request: PermissionRequest, action: string): boolean {
  const record = typeof request.input === "object" && request.input !== null ? request.input : {};
  return action.includes(title) || Object.values(record).some((v) => v === title);
}

/** The folder paths are judged against, and what to call it. */
function folderOf(request: PermissionRequest, project: Project | undefined): { path: string | null; name: string } {
  const path = request.cwd ?? project?.root ?? null;
  if (path === null) return { path, name: "a project" };
  const own = project !== undefined && sameFolder(path, project.root);
  return { path, name: own ? "the project folder" : "the agent's working folder" };
}
</script>

<section class="approvals" aria-labelledby="approvals-heading" data-testid="approvals">
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <h2
    id="approvals-heading"
    tabindex="-1"
    bind:this={heading}
    onkeydown={(event) => {
      if (event.code === "Escape") leave();
      else if (event.code === "ArrowDown") {
        event.preventDefault();
        cards()[0]?.focus();
      }
    }}
  >
    Approvals{#if queue.pending.length > 0}{" "}<span class="count">({queue.pending.length} waiting)</span>{/if}
  </h2>
  <p class="notice" aria-live="polite" data-testid="approvals-notice">{notice}</p>
  {#if queue.error}
    <p class="error" role="alert">{queue.error}</p>
  {/if}
  {#if queue.pending.length === 0}
    <p class="empty">Nothing waits for your approval.</p>
  {:else}
    <p class="hint" id="approvals-keys">On a request: Y allows, N denies, Shift+N denies with a message.</p>
  {/if}
  <ul bind:this={list}>
    {#each queue.pending as request (request.id)}
      {@const summary = describeTool(request.tool, request.input)}
      {@const ready = armed(request.id)}
      {@const domId = `approval-${request.id}`}
      {@const project = projects.get(request.project)}
      {@const folder = folderOf(request, project)}
      <li>
        <!-- A focusable group is how the keys reach one request (Y, N, arrows). -->
        <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
        <div
          class="card"
          role="group"
          tabindex="0"
          data-request={request.id}
          aria-labelledby="{domId}-action"
          aria-describedby="{domId}-session approvals-keys"
          onkeydown={(event) => onKeydown(event, request)}
        >
          <p class="session" id="{domId}-session">
            {sessionName(sessions.byId.get(request.session), request.session)} in {project?.name ?? "an unknown project"}
            asks to use {request.tool}
          </p>
          <h3 id="{domId}-action">{summary.action}</h3>
          {#if request.project !== workspace.project?.id}
            <p class="other" data-testid="approval-other-project">Not the project open in this window</p>
          {/if}
          {#if !project}
            <p class="scope outside" data-testid="approval-unknown-project">
              ⚠ Project unknown: Ovra cannot tell whether these paths are inside a project.
            </p>
          {/if}
          {#each pathScopes(request.input, folder.path) as scope}
            <p class="scope" class:outside={scope.scope !== "inside"} data-testid="approval-scope">
              {#if scope.scope === "inside"}Inside {folder.name}: {scope.relative}{#if scope.relative !== scope.path}{" "}<span
                    class="raw">({scope.path})</span
                  >{/if}{:else if scope.scope === "outside"}⚠ Outside {folder.name}: {scope.path}{:else}⚠ Cannot tell
                whether this is inside {folder.name}: {scope.path}{/if}
            </p>
          {/each}
          {#if request.tool === "Bash"}
            <p class="scope" data-testid="approval-scope">Shell commands are not checked for paths: read the command.</p>
          {/if}
          {#if request.reason}
            <p class="title" data-testid="approval-reason">
              Why it asks: {request.reason.text}
            </p>
          {/if}
          {#if request.title && !repeats(request.title, request, summary.action)}
            <p class="title">Agent's note: {request.title}</p>
          {/if}
          <dl>
            {#each summary.details as detail}
              <dt>{detail.label}</dt>
              <dd><pre>{detail.text}</pre></dd>
            {/each}
          </dl>
          <div class="actions">
            <button
              type="button"
              class="allow"
              aria-disabled={!ready}
              aria-keyshortcuts="Y"
              onclick={() => answer(request, "allow")}>Allow <kbd aria-hidden="true">Y</kbd></button
            >
            <button
              type="button"
              aria-disabled={!ready}
              aria-keyshortcuts="N"
              onclick={() => answer(request, "deny")}>Deny <kbd aria-hidden="true">N</kbd></button
            >
            <button type="button" aria-keyshortcuts="Shift+N" onclick={() => openDeny(request)}>Deny with message…</button>
          </div>
          {#if denying === request.id}
            <form
              onsubmit={(event) => {
                event.preventDefault();
                void answer(request, "deny", note.trim() || undefined);
              }}
            >
              <label for="{domId}-note">Message to the agent</label>
              <textarea id="{domId}-note" rows="2" bind:value={note}></textarea>
              <div class="actions">
                <button type="submit" aria-disabled={!ready} aria-keyshortcuts="Control+Enter">Send denial</button>
                <button type="button" onclick={() => (denying = null)}>Cancel</button>
              </div>
            </form>
          {/if}
        </div>
      </li>
    {/each}
  </ul>
</section>

<style>
  .approvals {
    padding: 8px 12px;
    border-bottom: 1px solid #2a2a31;
  }
  h2 {
    margin: 0 0 6px;
    font-size: 13px;
  }
  .count {
    color: #ffd36b;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-height: 50vh;
    overflow: auto;
  }
  .card {
    padding: 8px;
    border: 1px solid #ffd36b;
    border-radius: 4px;
    background: #1f1f26;
  }
  .card:focus-visible,
  h2:focus-visible {
    outline: 2px solid #7aa2ff;
    outline-offset: 2px;
  }
  h3 {
    margin: 2px 0 6px;
    font-size: 13px;
    overflow-wrap: anywhere;
  }
  .session,
  .title,
  .hint,
  .empty {
    margin: 0;
    color: #b4b4c0;
    font-size: 12px;
  }
  .hint {
    margin-bottom: 6px;
  }
  .scope {
    margin: 0 0 6px;
    font-size: 12px;
    overflow-wrap: anywhere;
  }
  .other {
    margin: 0 0 6px;
    font-size: 12px;
    font-weight: 600;
    color: #ffd36b;
  }
  .notice {
    margin: 0;
    font-size: 12px;
    color: #ffd36b;
  }
  .raw {
    color: #b4b4c0;
  }
  .scope.outside {
    font-weight: 600;
    color: #ffb86b;
  }
  dl {
    margin: 0 0 8px;
  }
  dt {
    color: #b4b4c0;
    font-size: 11px;
  }
  dd {
    margin: 0 0 4px;
  }
  pre {
    margin: 0;
    padding: 4px 6px;
    max-height: 160px;
    overflow: auto;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    background: #121216;
    border-radius: 3px;
    font-size: 12px;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  button {
    font: inherit;
    padding: 3px 10px;
    color: #e6e6e6;
    background: #2a2a33;
    border: 1px solid #4a4a57;
    border-radius: 3px;
    cursor: pointer;
  }
  button.allow {
    border-color: #6bd18a;
  }
  button[aria-disabled="true"] {
    opacity: 0.55;
    cursor: default;
  }
  button:focus-visible {
    outline: 2px solid #7aa2ff;
    outline-offset: 1px;
  }
  kbd {
    font: inherit;
    font-size: 11px;
    color: #b4b4c0;
  }
  form {
    margin-top: 8px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  textarea {
    font: inherit;
    color: inherit;
    background: #121216;
    border: 1px solid #4a4a57;
    border-radius: 3px;
  }
  .error {
    color: #ff8f8f;
    margin: 0 0 6px;
  }
  @media (forced-colors: active) {
    .card {
      border: 1px solid CanvasText;
    }
    button[aria-disabled="true"] {
      color: GrayText;
    }
  }
</style>
