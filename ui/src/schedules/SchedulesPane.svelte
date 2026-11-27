<script lang="ts">
// The project's Schedules pane (ADR 0015): its schedules, each with when it
// runs next and how the last run went, and a form that adds or changes one.
// Shown only while scheduled tasks are on (Settings > Agents).
import { tick, untrack } from "svelte";
import { providers } from "../agents/providers.svelte";
import type { AgentKind, Schedule, ScheduleKind } from "../lib/protocol";
import { CoreError } from "../lib/transport";
import { useOvra } from "../shell/context";
import EmptyState from "../shell/EmptyState.svelte";
import { schedules } from "./schedules.svelte";
import { DAYS, lastText, localZone, momentText, whenText } from "./words";

const { transport, workspace } = useOvra();
const store = schedules(transport);
const names = providers(transport);

const project = $derived(workspace.project?.id ?? null);
const list = $derived(project ? store.list(project) : []);
const agents = $derived(names.startable);

type Form = { name: string; prompt: string; kind: ScheduleKind; agent: AgentKind; days: number[]; time: string };
const blank = (): Form => ({
  name: "",
  prompt: "",
  kind: "research",
  agent: "claude",
  days: [1, 2, 3, 4, 5, 6, 7],
  time: "09:00",
});

let form = $state<Form>(blank());
/** The schedule being changed, or null for a new one; the form shows while `open`. */
let editing = $state<Schedule | null>(null);
let open = $state(false);
let saving = $state(false);
let error = $state<string | null>(null);
/** A row's last error (Run now, Pause, Remove), by schedule. */
let rowError = $state<Record<string, string>>({});
let now = $state(Date.now());
let pane = $state<HTMLElement>();

$effect(() => {
  const id = project;
  if (!id) return;
  untrack(() => void store.load(id).catch(() => undefined));
});

$effect(() => {
  const timer = setInterval(() => (now = Date.now()), 60_000);
  return () => clearInterval(timer);
});

function text(err: unknown): string {
  if (err instanceof CoreError) return err.error.message;
  return err instanceof Error ? err.message : String(err);
}

async function show(schedule: Schedule | null): Promise<void> {
  editing = schedule;
  error = null;
  form = schedule
    ? {
        name: schedule.name,
        prompt: schedule.prompt,
        kind: schedule.kind,
        agent: schedule.agent,
        days: [...schedule.when.days],
        time: schedule.when.time,
      }
    : blank();
  open = true;
  await tick();
  pane?.querySelector<HTMLElement>("[data-field=name]")?.focus();
}

function toggleDay(day: number): void {
  form.days = form.days.includes(day) ? form.days.filter((d) => d !== day) : [...form.days, day].sort();
}

async function save(event: SubmitEvent): Promise<void> {
  event.preventDefault();
  if (!project || saving) return;
  saving = true;
  error = null;
  try {
    const draft = {
      name: form.name,
      prompt: form.prompt,
      kind: form.kind,
      agent: form.agent,
      when: { days: form.days, time: form.time, time_zone: localZone() },
    };
    await store.save(project, draft, editing?.id);
    open = false;
    editing = null;
  } catch (err) {
    error = text(err);
  } finally {
    saving = false;
  }
}

async function act(schedule: Schedule, run: () => Promise<unknown>): Promise<void> {
  const { [schedule.id]: _, ...rest } = rowError;
  rowError = rest;
  try {
    await run();
  } catch (err) {
    rowError = { ...rowError, [schedule.id]: text(err) };
  }
}
</script>

<div class="schedules" data-testid="schedules-pane" bind:this={pane}>
  {#if list.length === 0 && !open}
    <div class="empty">
      <EmptyState
        headline="No schedules"
        line="A schedule starts an agent at the days and time you choose, for example to look for bugs every night."
        actions={[{ label: "Add schedule", run: () => void show(null) }]}
        testid="schedules-empty"
      />
    </div>
  {:else}
    <ul aria-label="Schedules">
      {#each list as schedule (schedule.id)}
        <li data-testid="schedule-row">
          <div class="head">
            <strong>{schedule.name}</strong>
            <span class="muted">{whenText(schedule.when)}</span>
            <span class="muted">{schedule.kind === "research" ? "Research" : "Own copy, then Reviews"}</span>
          </div>
          <p class="muted" data-testid="schedule-next">
            {schedule.paused ? "Paused" : schedule.next_ms ? `Next run ${momentText(schedule.next_ms, now)}` : "Not scheduled"}
          </p>
          {#if schedule.last}
            <p class="muted" data-testid="schedule-last">{lastText(schedule.last, now)}</p>
          {/if}
          {#if rowError[schedule.id]}<p class="error" role="alert">{rowError[schedule.id]}</p>{/if}
          <p class="actions-row">
            <button type="button" onclick={() => void act(schedule, () => store.runNow(schedule.id))}>Run now</button>
            <button type="button" onclick={() => void act(schedule, () => store.setPaused(schedule.id, !schedule.paused))}
              >{schedule.paused ? "Resume" : "Pause"}</button
            >
            <button type="button" onclick={() => void show(schedule)}>Edit</button>
            <button
              type="button"
              aria-label={`Remove ${schedule.name}`}
              onclick={() => void act(schedule, () => store.remove(schedule))}>Remove</button
            >
          </p>
        </li>
      {/each}
    </ul>
    {#if !open}
      <p class="actions-row">
        <button type="button" data-testid="schedules-add" onclick={() => void show(null)}>Add schedule…</button>
      </p>
    {/if}
  {/if}

  {#if open}
    <form onsubmit={save} aria-label={editing ? `Change ${editing.name}` : "New schedule"} data-testid="schedule-form">
      <label>
        Name
        <input bind:value={form.name} data-field="name" placeholder="For example Nightly bugs" autocomplete="off" />
      </label>
      <label>
        What to do
        <textarea bind:value={form.prompt} rows="3" placeholder="For example Look for bugs in the code changed yesterday"
        ></textarea>
      </label>
      <fieldset>
        <legend>Kind</legend>
        <label class="choice">
          <input type="radio" name="schedule-kind" value="research" bind:group={form.kind} />
          Research <span class="muted">Reads and reports, changes nothing.</span>
        </label>
        <label class="choice">
          <input type="radio" name="schedule-kind" value="own_copy" bind:group={form.kind} />
          One agent, own copy <span class="muted">Changes wait in Reviews.</span>
        </label>
      </fieldset>
      {#if agents.length > 1}
        <label>
          Agent
          <select bind:value={form.agent}>
            {#each agents as agent (agent)}<option value={agent}>{names.name(agent)}</option>{/each}
          </select>
        </label>
      {/if}
      <fieldset>
        <legend>Days</legend>
        <div class="days">
          {#each DAYS as day, at (day)}
            <label class="day">
              <input type="checkbox" checked={form.days.includes(at + 1)} onchange={() => toggleDay(at + 1)} />
              {day}
            </label>
          {/each}
        </div>
      </fieldset>
      <label>
        Time
        <input type="time" bind:value={form.time} required data-field="time" />
      </label>
      {#if error}<p class="error" role="alert" data-testid="schedule-error">{error}</p>{/if}
      <p class="actions-row">
        <button type="submit" class="primary" aria-busy={saving}>{editing ? "Save" : "Add schedule"}</button>
        <button type="button" onclick={() => ((open = false), (editing = null), (error = null))}>Cancel</button>
      </p>
    </form>
  {/if}
</div>

<style>
  .schedules {
    max-width: 560px;
  }
  .muted,
  label,
  legend {
    color: var(--text-2);
    font: var(--t-small);
  }
  .empty > :global(.empty) {
    padding: 0 0 8px;
  }
  ul {
    list-style: none;
    display: grid;
    gap: 4px;
    margin: 0 0 8px;
    padding: 0;
  }
  li {
    display: grid;
    gap: 2px;
    padding: 8px 10px;
    background: var(--s1);
    border-radius: var(--r-md);
  }
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 8px;
  }
  p {
    margin: 0;
  }
  .actions-row {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-top: 4px;
  }
  form {
    display: grid;
    gap: 10px;
    margin-top: 8px;
  }
  form > label {
    display: grid;
    gap: 4px;
  }
  fieldset {
    margin: 0;
    padding: 0;
    border: 0;
    display: grid;
    gap: 4px;
  }
  .choice,
  .day {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--text);
  }
  .choice .muted {
    margin-left: 4px;
  }
  .days {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
  }
  button,
  input:not([type="checkbox"], [type="radio"]),
  select,
  textarea {
    font: inherit;
    padding: 5px 10px;
    color: var(--text);
    background: var(--s3);
    border: 0;
    border-radius: var(--r-md);
  }
  textarea {
    resize: vertical;
  }
  button {
    font-weight: 500;
    cursor: default;
  }
  button:hover {
    background: var(--s4);
  }
  button.primary {
    color: var(--accent-ink);
    background: var(--accent);
  }
  .error {
    color: var(--danger);
  }
</style>
