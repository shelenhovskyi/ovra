<script lang="ts">
// Toasts for agent events, announced through live regions without taking
// focus. Requests for approval and errors are announced at once, the rest
// politely.
import { approvalQueue } from "../approvals/queue.svelte";
import { registerCommand } from "../keys/commands";
import type { PermissionId } from "../lib/protocol";
import { useOvra } from "../shell/context";
import { watchAgents } from "./agent-events";
import { Notifier, type SystemNotifications, type Toast } from "./notifier.svelte";

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
        granted: "System notifications are on: Ovra tells you when an agent needs you while it is in the background.",
        denied: "System notifications are blocked. Allow them in the browser or system settings.",
        default: "System notifications stay off.",
        unsupported: "System notifications are not available here.",
      }[answer];
      notifier.notify({ key: "system-permission", level: answer === "granted" ? "info" : "error", text });
    },
  }),
);

const urgent = $derived(notifier.toasts.filter((t) => t.level !== "info"));
const polite = $derived(notifier.toasts.filter((t) => t.level === "info"));
</script>

{#snippet toast(item: Toast)}
  <div class="toast {item.level}" data-testid="toast">
    <p>{item.text}</p>
    <div class="actions">
      {#if item.action}
        {@const action = item.action}
        <button
          type="button"
          onclick={() => {
            notifier.dismiss(item.id);
            action.run();
          }}>{action.label}</button
        >
      {/if}
      <button type="button" aria-label="Dismiss notification" onclick={() => notifier.dismiss(item.id)}>×</button>
    </div>
  </div>
{/snippet}

<div class="notifications" data-testid="notifications">
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
    gap: 6px;
    width: min(360px, calc(100vw - 32px));
    pointer-events: none;
  }
  .notifications > div {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .toast {
    pointer-events: auto;
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 8px 10px;
    background: #24242c;
    border: 1px solid #4a4a57;
    border-left-width: 4px;
    border-radius: 4px;
    box-shadow: 0 4px 12px rgb(0 0 0 / 40%);
  }
  .attention {
    border-left-color: #ffd36b;
  }
  .error {
    border-left-color: #ff8f8f;
  }
  p {
    margin: 0;
    flex: 1;
    overflow-wrap: anywhere;
  }
  .actions {
    display: flex;
    gap: 4px;
  }
  button {
    font: inherit;
    padding: 2px 8px;
    color: #e6e6e6;
    background: #2a2a33;
    border: 1px solid #4a4a57;
    border-radius: 3px;
    cursor: pointer;
  }
  button:focus-visible {
    outline: 2px solid #7aa2ff;
    outline-offset: 1px;
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
