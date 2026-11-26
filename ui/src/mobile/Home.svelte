<script lang="ts">
import { statusLabel } from "../agents/status";
import { describeTool } from "../approvals/describe";
import type { SessionInfo } from "../lib/protocol";
import Header from "./Header.svelte";
import { finished, type Phone, working } from "./phone.svelte";
import { href } from "./route";

let { phone }: { phone: Phone } = $props();

const pending = $derived(phone.approvals.pending);
const busy = $derived(working(phone));
const done = $derived(finished(phone));
const awake = $derived(phone.power?.mode !== undefined && phone.power.mode !== "off");

function where(session: SessionInfo | undefined, project: string): string {
  const name = phone.project(project)?.name ?? "Unknown project";
  return session ? `${session.title} · ${name}` : name;
}
</script>

<Header title="Ovra" status={phone.status} back={false} link={{ href: href({ screen: "computer" }), text: "Computer" }} />

{#if phone.error}<p class="error" role="alert">{phone.error}</p>{/if}
{#if phone.approvals.error}<p class="error" role="alert">{phone.approvals.error}</p>{/if}

<section aria-labelledby="needs-you">
  <h2 id="needs-you">Needs you</h2>
  {#if pending.length === 0}
    <p class="empty">Nothing waits for you.</p>
  {:else}
    <ul data-testid="needs-you">
      {#each pending as request (request.id)}
        <li>
          <a class="row" href={href({ screen: "approval", id: request.id })}>
            <span class="what">{describeTool(request.tool, request.input, request.call).action}</span>
            <span class="who">{where(phone.session(request.session), request.project)}</span>
          </a>
        </li>
      {/each}
    </ul>
  {/if}
</section>

<section aria-labelledby="working">
  <h2 id="working">Working</h2>
  {#if busy.length === 0}
    <p class="empty">No agent works right now.</p>
  {:else}
    <ul data-testid="working">
      {#each busy as session (session.id)}
        <li>
          <a class="row" href={href({ screen: "agent", session: session.id })}>
            <span class="what">{session.title}</span>
            <span class="who">{statusLabel(session.status)} · {phone.project(session.project)?.name ?? ""}</span>
          </a>
        </li>
      {/each}
    </ul>
  {/if}
</section>

{#if phone.tasks.length > 0}
  <section aria-labelledby="tasks">
    <h2 id="tasks">Tasks</h2>
    <ul>
      {#each phone.tasks as task (task.id)}
        <li>
          <a class="row" href={href({ screen: "task", id: task.id })}>
            <span class="what">{phone.session(task.coordinator ?? task.workers[0]?.session ?? "")?.title ?? "Task"}</span>
            <span class="who"
              >{task.workers.length === 1 ? "1 worker" : `${task.workers.length} workers`} · {phone.project(task.project)
                ?.name ?? ""}</span
            >
          </a>
        </li>
      {/each}
    </ul>
  </section>
{/if}

{#if done.length > 0}
  <section aria-labelledby="finished">
    <h2 id="finished">Finished</h2>
    <ul data-testid="finished">
      {#each done as session (session.id)}
        <li>
          <a class="row" href={href({ screen: "agent", session: session.id })}>
            <span class="what">{session.title}</span>
            <span class="who">{statusLabel(session.status)} · {phone.project(session.project)?.name ?? ""}</span>
          </a>
        </li>
      {/each}
    </ul>
  </section>
{/if}

{#if phone.power}
  <label class="switch">
    <input
      type="checkbox"
      role="switch"
      checked={awake}
      onchange={(e) => void phone.setMode(e.currentTarget.checked ? "while_agents_work" : "off")}
    />
    <span>Keep the computer awake while agents work</span>
  </label>
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
  .empty {
    color: var(--text-2);
  }
  .error {
    color: var(--danger);
  }
  .switch {
    display: flex;
    gap: var(--sp-3);
    align-items: center;
    min-height: 44px;
    margin-top: var(--sp-6);
  }
  .switch input {
    width: 24px;
    height: 24px;
  }
</style>
