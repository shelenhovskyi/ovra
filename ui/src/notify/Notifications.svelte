<script lang="ts">
// Toasts for agent events, announced through live regions without taking
// focus. Requests for approval and errors are announced at once, the rest
// politely. The approvals toast steps aside while an approval card is on
// screen; while a modal covers the window, a banner over it says that
// approvals wait, so a request is never hidden.
import { approvalQueue } from "../approvals/queue.svelte";
import { registerCommand } from "../keys/commands";
import type { PermissionId } from "../lib/protocol";
import { useOvra } from "../shell/context";
import { APPROVALS_KEY, watchAgents } from "./agent-events";
import { Notifier, overlaps, type SystemNotifications, type Toast } from "./notifier.svelte";

let { system }: { system?: SystemNotifications } = $props();

const { transport } = useOvra();
// svelte-ignore state_referenced_locally
const notifier = new Notifier(system);

/** Focuses the request the notification is about, not just the first one. */
function review(id: PermissionId): void {
  approvals.reveal(id);
}

$effect(() => watchAgents(transport, notifier, review));

// Seen from another tab or window too: "(2) Ovra" while two requests wait.
const approvals = approvalQueue(transport);
$effect(() => {
  const waiting = approvals.pending.length;
  const base = document.title.replace(/^\(\d+\) /, "");
  document.title = waiting > 0 ? `(${waiting}) ${base}` : base;
  return () => {
    document.title = base;
  };
});

$effect(() =>
  registerCommand({
    id: "notify.enable_system",
    title: "Turn on system notifications",
    run: async () => {
      const answer = await notifier.system.request();
      const text = {
        granted: "System notifications on: Ovra tells you when an agent needs you.",
        denied: "System notifications blocked. Allow them in system settings.",
        default: "System notifications stay off.",
        unsupported: "System notifications are not available here.",
      }[answer];
      notifier.notify({ key: "system-permission", level: answer === "granted" ? "info" : "error", text });
    },
  }),
);

/** Bumped when what is on screen may have changed (focus moved, as it does when a modal opens or closes; a resize), so the checks below run again. */
let looked = $state(0);
const look = () => looked++;

/** An approval card is in sight: no toast for approvals then, and no banner. */
function cardInSight(): boolean {
  void looked;
  for (const card of document.querySelectorAll<HTMLElement>("[data-testid=approvals] [data-request]")) {
    const box = card.getBoundingClientRect();
    if (box.bottom > 0 && box.top < window.innerHeight && box.height > 0 && !card.closest("[hidden]")) return true;
  }
  return false;
}

/** The open modal, if any: it covers the approvals. */
function modal(): HTMLElement | null {
  void looked;
  return document.querySelector<HTMLElement>("[role=dialog][aria-modal=true]");
}

const covered = $derived(approvals.pending.length > 0 && modal() !== null);
const shown = $derived(
  notifier.toasts.filter((t) => t.key !== APPROVALS_KEY || (approvals.focused === null && !covered && !cardInSight())),
);

/** Closes the modal (as Escape would) and shows the oldest request. */
function showApprovals(): void {
  modal()?.dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", key: "Escape", bubbles: true }));
  const first = approvals.pending[0];
  if (first) setTimeout(() => review(first.id));
}
const urgent = $derived(shown.filter((t) => t.level !== "info"));
const polite = $derived(shown.filter((t) => t.level === "info"));

/** The gap under the stack, as in the styles. */
const BOTTOM = 32;
let stack = $state<HTMLElement>();
/** The stack moves to the top while, at the bottom, it would cover what has focus (WCAG 2.4.11). */
let atTop = $state(false);

function place(): void {
  const focus = document.activeElement;
  if (!stack || stack.contains(focus)) return;
  const box = stack.getBoundingClientRect();
  const bottom = window.innerHeight - BOTTOM;
  const there = { left: box.left, right: box.right, top: bottom - box.height, bottom };
  atTop = focus instanceof HTMLElement && focus !== document.body && overlaps(there, focus.getBoundingClientRect());
}

// Again whenever the stack changes size, focus moves or the window resizes.
$effect(() => {
  void shown.length;
  place();
});

$effect(() => {
  const again = () => {
    place();
    look();
  };
  document.addEventListener("focusin", again);
  window.addEventListener("resize", again);
  return () => {
    document.removeEventListener("focusin", again);
    window.removeEventListener("resize", again);
  };
});
</script>

{#snippet toast(item: Toast)}
  <div class="toast {item.level}" data-testid="toast">
    <span class="dot" aria-hidden="true"></span>
    <p>{item.text}</p>
    <div class="actions">
      {#if item.action}
        {@const action = item.action}
        <button
          type="button"
          class="act"
          onclick={() => {
            notifier.dismiss(item.id);
            action.run();
          }}>{action.label}</button
        >
      {/if}
      <button type="button" aria-label="Dismiss notification" onclick={() => notifier.dismiss(item.id)}
        ><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg></button
      >
    </div>
  </div>
{/snippet}

{#if covered}
  {@const count = approvals.pending.length}
  <div class="banner" role="status" data-testid="approvals-banner">
    <span class="dot" aria-hidden="true"></span>
    <p>{count === 1 ? "1 approval waits" : `${count} approvals wait`}</p>
    <button type="button" class="act" onclick={showApprovals}>Show</button>
  </div>
{/if}
<div class="notifications" class:top={atTop} data-testid="notifications" bind:this={stack}>
  <div aria-live="assertive">
    {#each urgent as item (item.id)}{@render toast(item)}{/each}
  </div>
  <div aria-live="polite">
    {#each polite as item (item.id)}{@render toast(item)}{/each}
  </div>
</div>

<style>
  .notifications {
    position: fixed;
    right: 352px;
    bottom: 32px;
    z-index: 10;
    display: flex;
    flex-direction: column;
    gap: 8px;
    width: min(380px, calc(100vw - 32px));
    pointer-events: none;
  }
  /* Under the header, over the main area. */
  .notifications.top {
    top: 44px;
    bottom: auto;
  }
  .notifications > div {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  /* Over any modal and its scrim: an approval is never hidden. */
  .banner {
    position: fixed;
    top: 8px;
    left: 50%;
    z-index: 1000;
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: 10px;
    height: 32px;
    padding: 0 6px 0 14px;
    color: var(--text);
    background: var(--s4);
    border-radius: 16px;
    box-shadow: var(--card-shadow);
  }
  .banner .dot {
    background: var(--warn);
  }
  .toast {
    pointer-events: auto;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 8px 8px 14px;
    color: var(--text);
    background: var(--s4);
    border-radius: 10px;
    box-shadow: var(--card-shadow);
  }
  .dot {
    flex: none;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--accent);
  }
  .attention .dot {
    background: var(--warn);
  }
  .error .dot {
    background: var(--danger);
  }
  p {
    margin: 0;
    flex: 1;
    overflow-wrap: anywhere;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 2px;
  }
  button {
    display: grid;
    place-items: center;
    min-width: 28px;
    font: inherit;
    padding: 3px 8px;
    color: var(--text-2);
    background: transparent;
    border: 0;
    border-radius: var(--radius-sm);
    cursor: pointer;
  }
  button:hover {
    color: var(--text);
    background: var(--s3);
  }
  button.act {
    color: var(--accent);
    font-weight: 500;
  }
  svg {
    width: 15px;
    height: 15px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.6;
    stroke-linecap: round;
  }
  @media (max-width: 900px) {
    .notifications {
      right: 16px;
    }
  }
  @media (forced-colors: active) {
    .toast {
      border: 1px solid CanvasText;
    }
  }
</style>
