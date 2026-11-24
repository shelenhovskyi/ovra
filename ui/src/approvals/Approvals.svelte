<script lang="ts">
// Pending permission requests, oldest first. Keyboard: Alt+Shift+A (Mod+Shift+A
// in the desktop app) jumps here; on a focused card Y allows, N denies,
// Shift+N denies with a message, arrows move between cards, Escape goes back.
// Focus never moves here from elsewhere and nothing is answered without a press.
import { tick } from "svelte";
import { agentSessions, sessionName } from "../agents/sessions.svelte";
import { registerCommand } from "../keys/commands";
import type { Decision, PermissionRequest } from "../lib/protocol";
import { useOvra } from "../shell/context";
import { describeTool } from "./describe";
import { ARM_MS, approvalQueue } from "./queue.svelte";

const { transport } = useOvra();
const queue = approvalQueue(transport);
const sessions = agentSessions(transport);

let now = $state(Date.now());
/** Set on every answer: the next request waits ARM_MS from here too. */
let lastAnswer = $state(0);
let denying = $state<string | null>(null);
let note = $state("");
let list = $state<HTMLElement>();
let heading = $state<HTMLElement>();
let returnTo: HTMLElement | null = null;

const armedAt = (id: string): number => Math.max(queue.arrived.get(id) ?? 0, lastAnswer) + ARM_MS;
const armed = (id: string): boolean => now >= armedAt(id) && !queue.answering.has(id);

// Re-render when the next request becomes answerable.
$effect(() => {
  const waits = queue.pending.map((r) => armedAt(r.id)).filter((t) => t > now);
  if (waits.length === 0) return;
  const timer = setTimeout(() => (now = Date.now()), Math.min(...waits) - now);
  return () => clearTimeout(timer);
});

$effect(() =>
  registerCommand({ id: "approvals.focus", title: "Go to pending approvals", keys: "Mod+Shift+A", run: enter }),
);

function cards(): HTMLElement[] {
  return [...(list?.querySelectorAll<HTMLElement>("[data-request]") ?? [])];
}

function enter(): void {
  const active = document.activeElement;
  if (active instanceof HTMLElement && !list?.contains(active) && active !== heading) returnTo = active;
  (cards()[0] ?? heading)?.focus();
}

function leave(): void {
  if (returnTo?.isConnected) returnTo.focus();
  returnTo = null;
}

async function answer(request: PermissionRequest, decision: Decision, message?: string): Promise<void> {
  if (Date.now() < armedAt(request.id) || queue.answering.has(request.id)) return;
  const index = queue.pending.indexOf(request);
  const focused = list?.contains(document.activeElement) ?? false;
  lastAnswer = Date.now();
  now = lastAnswer;
  const sent = await queue.answer(request.id, decision, message);
  if (!sent) return;
  if (denying === request.id) {
    denying = null;
    note = "";
  }
  await tick();
  if (!focused) return;
  const next = cards();
  const target = next[Math.min(index, next.length - 1)];
  // With nothing left, stay here: the next request is focused when it arrives.
  (target ?? heading)?.focus();
}

// Someone waiting on the empty list gets the next request focused. Focus
// never jumps here from elsewhere.
$effect(() => {
  if (queue.pending.length > 0 && heading && document.activeElement === heading)
    void tick().then(() => cards()[0]?.focus());
});

async function openDeny(request: PermissionRequest): Promise<void> {
  denying = request.id;
  note = "";
  await tick();
  list?.querySelector<HTMLTextAreaElement>(`[data-request="${request.id}"] textarea`)?.focus();
}

function onKeydown(event: KeyboardEvent, request: PermissionRequest): void {
  if (event.target instanceof HTMLTextAreaElement) {
    if (event.code === "Escape") {
      event.preventDefault();
      denying = null;
      list?.querySelector<HTMLElement>(`[data-request="${request.id}"]`)?.focus();
    } else if (event.code === "Enter" && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      void answer(request, "deny", note.trim() || undefined);
    }
    return;
  }
  if (event.ctrlKey || event.metaKey || event.altKey || event.isComposing) return;
  const all = cards();
  const at = all.findIndex((card) => card.dataset.request === request.id);
  const handled = (() => {
    switch (event.code) {
      case "KeyY":
        if (event.shiftKey) return false;
        void answer(request, "allow");
        return true;
      case "KeyN":
        if (event.shiftKey) void openDeny(request);
        else void answer(request, "deny");
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
</script>

<section class="approvals" aria-labelledby="approvals-heading" data-testid="approvals">
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <h2
    id="approvals-heading"
    tabindex="-1"
    bind:this={heading}
    onkeydown={(event) => {
      if (event.code === "Escape") leave();
    }}
  >
    Approvals{#if queue.pending.length > 0}{" "}<span class="count">({queue.pending.length} waiting)</span>{/if}
  </h2>
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
            {sessionName(sessions.byId.get(request.session), request.session)} asks to use {request.tool}
          </p>
          <h3 id="{domId}-action">{summary.action}</h3>
          {#if request.title && request.title !== summary.action}
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
