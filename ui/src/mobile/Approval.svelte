<script lang="ts">
// One request, with Deny and Allow. Allow once only: "Always allow" changes
// the project's rules and stays on the computer (ADR 0014). Both buttons
// wait ARM_MS after the request and the screen appear, so the tap that
// opened the screen cannot answer it.
import { onDestroy } from "svelte";
import { describeTool } from "../approvals/describe";
import { ARM_MS } from "../approvals/queue.svelte";
import Header from "./Header.svelte";
import type { Phone } from "./phone.svelte";
import { href } from "./route";

let { phone, id }: { phone: Phone; id: string } = $props();

const request = $derived(phone.approvals.pending.find((r) => r.id === id));
const summary = $derived(request ? describeTool(request.tool, request.input, request.call) : null);
const session = $derived(request ? phone.session(request.session) : undefined);

let armed = $state(false);
let denying = $state(false);
let note = $state("");
let answered = $state<"allow" | "deny" | null>(null);
const shownAt = Date.now();
let timer: ReturnType<typeof setTimeout> | undefined;

$effect(() => {
  if (!request) return;
  const since = Math.max(phone.approvals.arrived.get(request.id) ?? shownAt, shownAt);
  armed = false;
  clearTimeout(timer);
  timer = setTimeout(() => (armed = true), Math.max(0, since + ARM_MS - Date.now()));
});
onDestroy(() => clearTimeout(timer));

async function answer(decision: "allow" | "deny"): Promise<void> {
  if (!request || !armed) return;
  const asked = request;
  const sent = await phone.approvals.answer(
    asked.id,
    decision,
    decision === "deny" ? note.trim() || undefined : undefined,
  );
  if (!sent) return;
  answered = decision;
  // The same agent's next request, if it asks again; never another agent's.
  const next = phone.approvals.pending.find((r) => r.session === asked.session);
  location.hash = next ? href({ screen: "approval", id: next.id }) : href({ screen: "home" });
}
</script>

<Header title="Approval" status={phone.status} />

{#if request && summary}
  <article aria-labelledby="request-title" data-testid="approval">
    <p class="who">
      {session?.title ?? "An agent"} · {phone.project(request.project)?.name ?? "Unknown project"}
    </p>
    <h2 id="request-title">{summary.action}</h2>
    {#if request.reason}<p class="reason">{request.reason.text}</p>{/if}
    {#each summary.details as detail (detail.label)}
      <section class="detail">
        <h3>{detail.label}</h3>
        <pre>{detail.text}</pre>
      </section>
    {/each}

    {#if phone.approvals.error}<p class="error" role="alert">{phone.approvals.error}</p>{/if}

    {#if denying}
      <form
        onsubmit={(e) => {
          e.preventDefault();
          void answer("deny");
        }}
      >
        <label for="deny-note">Tell the agent why (optional)</label>
        <textarea id="deny-note" rows="3" bind:value={note}></textarea>
        <div class="actions">
          <button type="button" onclick={() => (denying = false)}>Cancel</button>
          <button type="submit" class="danger" aria-disabled={!armed}>Deny</button>
        </div>
      </form>
    {:else}
      <div class="actions">
        <button type="button" class="danger" aria-disabled={!armed} onclick={() => void answer("deny")}>Deny</button>
        <button type="button" class="primary" aria-disabled={!armed} onclick={() => void answer("allow")}>Allow</button>
      </div>
      <button type="button" class="quiet" onclick={() => (denying = true)}>Deny with a reason…</button>
      <p class="hint">"Always allow" is only on your computer.</p>
    {/if}
  </article>
{:else if answered === null}
  <p role="status">This request was answered or its agent ended.</p>
  <a href="#/">Back to Home</a>
{/if}

<style>
  .who,
  .reason,
  .hint {
    color: var(--text-2);
  }
  h2 {
    font: var(--t-title2);
    overflow-wrap: anywhere;
  }
  h3 {
    font: var(--t-caption);
    color: var(--text-2);
    text-transform: capitalize;
    margin: var(--sp-3) 0 var(--sp-1);
  }
  pre {
    font: 400 13px / 18px var(--mono);
    background: var(--s2);
    padding: var(--sp-3);
    border-radius: var(--r-md);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    max-height: 40vh;
    overflow: auto;
    margin: 0;
  }
  .actions {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--sp-3);
    margin-top: var(--sp-5);
  }
  .quiet {
    width: 100%;
    margin-top: var(--sp-3);
    background: transparent;
    border-color: transparent;
    color: var(--accent);
  }
  form {
    display: grid;
    gap: var(--sp-2);
    margin-top: var(--sp-5);
  }
  .error {
    color: var(--danger);
  }
</style>
