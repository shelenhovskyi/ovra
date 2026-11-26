<script lang="ts">
// A task: its workers and what they gave for review, to read. Keep and
// Discard stay on the computer, where the changes can be seen (ADR 0014).
import { statusLabel } from "../agents/status";
import type { ReviewState } from "../lib/protocol";
import Header from "./Header.svelte";
import type { Phone } from "./phone.svelte";
import { href } from "./route";

let { phone, id }: { phone: Phone; id: string } = $props();

const task = $derived(phone.tasks.find((t) => t.id === id));
const sessions = $derived(new Set(task ? [task.coordinator, ...task.workers.map((w) => w.session)] : []));
const reviews = $derived(phone.reviews.filter((r) => sessions.has(r.session)));

const REVIEW: Record<ReviewState, string> = {
  open: "Waits for your review",
  approved: "Kept",
  partly_approved: "Partly kept",
  rejected: "Discarded",
  published: "Pull request open",
};
</script>

<!-- One agent in its own copy has no coordinator: its one worker names the task. -->
<Header
  title={task ? (phone.session(task.coordinator ?? task.workers[0]?.session ?? "")?.title ?? "Task") : "Task"}
  status={phone.status}
/>

{#if task}
  <p class="where">{phone.project(task.project)?.name ?? ""}</p>
  {#if task.coordinator}
    <a class="row" href={href({ screen: "agent", session: task.coordinator })}>
      <span class="what">Coordinator</span>
      <span class="who">{statusLabel(phone.session(task.coordinator)?.status ?? { state: "idle" })}</span>
    </a>
  {/if}

  <h2>Workers</h2>
  {#if task.workers.length === 0 && !task.failed?.length}
    <p class="who">No workers yet.</p>
  {/if}
  <ul data-testid="workers">
    {#each task.workers as worker (worker.session)}
      {@const session = phone.session(worker.session)}
      <li>
        <a class="row" href={href({ screen: "agent", session: worker.session })}>
          <span class="what">{worker.name}</span>
          <span class="who">{worker.waiting ?? (session ? statusLabel(session.status) : "")}</span>
        </a>
      </li>
    {/each}
    {#each task.failed ?? [] as failed (failed.name)}
      <li class="row">
        <span class="what">{failed.name}</span>
        <span class="who">Did not start: {failed.reason}</span>
      </li>
    {/each}
  </ul>

  {#if reviews.length > 0}
    <h2>For review</h2>
    <ul data-testid="reviews">
      {#each reviews as item (item.id)}
        <li class="row">
          <span class="what">{item.summary ?? phone.session(item.session)?.title ?? "Changes"}</span>
          <span class="who"
            >{REVIEW[item.state]} · {item.files.length === 1 ? "1 file" : `${item.files.length} files`}</span
          >
        </li>
      {/each}
    </ul>
    <p class="who">Keep or discard changes on your computer, where you can see them.</p>
  {/if}
{:else if phone.loaded}
  <p role="status">This task is not on your computer any more.</p>
{/if}

<style>
  h2 {
    font: var(--t-title2);
    margin: var(--sp-6) 0 var(--sp-2);
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: var(--sp-2);
  }
  .row {
    display: grid;
    padding: var(--sp-3);
    background: var(--s2);
    border-radius: var(--r-lg);
    color: var(--text);
    text-decoration: none;
  }
  .what {
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .who,
  .where {
    color: var(--text-2);
  }
</style>
