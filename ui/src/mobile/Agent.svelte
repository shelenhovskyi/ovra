<script lang="ts">
// One agent: a compact conversation (what was said and one line per step),
// a reply box and Stop. Starting or resuming agents stays on the computer.
import { isBusy, isEnded, statusLabel } from "../agents/status";
import { describeTool } from "../approvals/describe";
import type { AgentMessage } from "../lib/protocol";
import { renderPhoneChat } from "./chat";
import Header from "./Header.svelte";
import type { Phone } from "./phone.svelte";
import { href } from "./route";

let { phone, session: id }: { phone: Phone; session: string } = $props();

/** Only the latest messages; the full log is on the computer. */
const SHOWN = 60;

const session = $derived(phone.session(id));
const messages = $derived(phone.transcripts.get(id) ?? []);
const shown = $derived(messages.slice(-SHOWN));
const asking = $derived(phone.approvals.pending.filter((r) => r.session === id));
let text = $state("");
let sending = $state(false);

$effect(() => {
  if (phone.status === "open" && !phone.transcripts.has(id)) void phone.loadTranscript(id);
});

type Line = { key: string; kind: "you" | "agent" | "step" | "error"; text: string };

function lines(list: AgentMessage[]): Line[] {
  const out: Line[] = [];
  list.forEach((m, i) => {
    const key = String(messages.length - list.length + i);
    if (m.kind === "user_text") out.push({ key, kind: "you", text: m.text });
    else if (m.kind === "assistant_text" && !m.parent) out.push({ key, kind: "agent", text: m.text });
    else if (m.kind === "tool_use" && !m.parent)
      out.push({ key, kind: "step", text: describeTool(m.name, m.input, m.call).action });
    else if (m.kind === "error") out.push({ key, kind: "error", text: m.message });
    else if (m.kind === "interrupted") out.push({ key, kind: "step", text: "Stopped by you" });
  });
  return out;
}

async function send(event: SubmitEvent): Promise<void> {
  event.preventDefault();
  const reply = text.trim();
  if (!reply || sending) return;
  sending = true;
  if (await phone.reply(id, reply)) text = "";
  sending = false;
}
</script>

<Header title={session?.title ?? "Agent"} status={phone.status} />

{#if session}
  <p class="state" data-testid="agent-state">{statusLabel(session.status)} · {phone.project(session.project)?.name ?? ""}</p>
  {#each asking as request (request.id)}
    <a class="ask" href={href({ screen: "approval", id: request.id })}
      >Needs you: {describeTool(request.tool, request.input, request.call).action}</a
    >
  {/each}

  <ol class="log" aria-label="Conversation">
    {#if messages.length > SHOWN}<li class="more">Earlier messages are on your computer.</li>{/if}
    {#each lines(shown) as line (line.key)}
      <li class={line.kind}>
        {#if line.kind === "agent"}
          <!-- renderPhoneChat keeps only safe tags and attributes, and no links. -->
          <div class="md">{@html renderPhoneChat(line.text)}</div>
        {:else}
          {line.text}
        {/if}
      </li>
    {/each}
  </ol>

  {#if phone.error}<p class="error" role="alert">{phone.error}</p>{/if}

  {#if !isEnded(session.status)}
    <form onsubmit={send}>
      <label for="reply">Reply</label>
      <textarea id="reply" rows="3" bind:value={text}></textarea>
      <div class="actions">
        {#if isBusy(session.status)}
          <button type="button" class="danger" onclick={() => void phone.interrupt(id)}>Stop</button>
        {:else}
          <button type="button" class="danger" onclick={() => void phone.stop(id)}>End agent</button>
        {/if}
        <button type="submit" class="primary" aria-disabled={sending || !text.trim()}>Send</button>
      </div>
    </form>
  {/if}
{:else if phone.loaded}
  <p role="status">This agent is not on your computer any more.</p>
{/if}

<style>
  .state {
    color: var(--text-2);
  }
  .ask {
    display: block;
    padding: var(--sp-3);
    border-radius: var(--r-lg);
    background: var(--warn-soft);
    color: var(--warn);
    font-weight: 600;
    text-decoration: none;
    margin-bottom: var(--sp-2);
  }
  .log {
    list-style: none;
    padding: 0;
    margin: var(--sp-4) 0;
    display: grid;
    gap: var(--sp-2);
  }
  .log li {
    overflow-wrap: anywhere;
  }
  .you {
    justify-self: end;
    background: var(--accent-soft);
    padding: var(--sp-2) var(--sp-3);
    border-radius: var(--r-lg);
    white-space: pre-wrap;
  }
  .step,
  .more {
    color: var(--text-2);
    font: var(--t-small);
  }
  .error {
    color: var(--danger);
  }
  .md :global(pre) {
    white-space: pre-wrap;
    font: 400 13px / 18px var(--mono);
  }
  .md :global(a) {
    pointer-events: none;
    color: var(--accent);
  }
  form {
    display: grid;
    gap: var(--sp-2);
  }
  label {
    font: var(--t-headline);
  }
  .actions {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--sp-3);
  }
</style>
