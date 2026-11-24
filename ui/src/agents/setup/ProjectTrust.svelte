<script lang="ts">
// Per-project trust. Until the user trusts a project, Claude runs with the
// user's own settings only; trusting it also loads the project's .claude/
// settings, hooks, agents, commands and skills. Hooks and helpers run
// commands, so the card shows everything that would load before asking.
// Settings that changed since (a git pull, say) do not load until the user
// reviews what changed and trusts them again.
import { untrack } from "svelte";
import type { ProjectConfigReview } from "../../lib/protocol";
import { CoreError } from "../../lib/transport";
import { useOvra } from "../../shell/context";
import { changes, KIND_LABELS, runsCode } from "./texts";

const { transport, workspace } = useOvra();

let review = $state<ProjectConfigReview | null>(null);
let reviewing = $state(false);
let busy = $state(false);
let error = $state<string | null>(null);

const project = $derived(workspace.project);
const changed = $derived(
  project?.trusted === true && review !== null && review.trusted?.fingerprint !== review.current.fingerprint,
);
const diff = $derived(changed && review?.trusted ? changes(review.trusted, review.current) : []);
const items = $derived(
  [...(review?.current.items ?? [])].sort((a, b) => Number(runsCode(b.kind)) - Number(runsCode(a.kind))),
);
const risky = $derived(
  changed ? diff.some((c) => runsCode(c.kind) && c.added.length > 0) : items.some((item) => runsCode(item.kind)),
);
const heading = $derived(
  !project?.trusted
    ? "Project settings: not loaded"
    : changed
      ? "Project settings changed since you trusted them"
      : "Project settings: trusted",
);

$effect(() => {
  const id = project?.id;
  untrack(() => {
    reviewing = false;
    review = null;
    error = null;
    if (id) void load(id);
  });
});

// A session start is when changed settings matter: check again then.
$effect(() =>
  transport.onEvent((event) => {
    if (event.type !== "agents.status" || event.data.status.state !== "starting") return;
    if (event.data.project === project?.id) void load(event.data.project);
  }),
);

function text(err: unknown): string {
  if (err instanceof CoreError) return err.error.message;
  return err instanceof Error ? err.message : String(err);
}

async function load(id: string): Promise<void> {
  try {
    const answer = await transport.call({ method: "agents.project_config", params: { project: id } });
    if (workspace.project?.id === id) review = answer.data;
  } catch (err) {
    error = text(err);
  }
}

async function run(action: () => Promise<void>): Promise<void> {
  if (busy) return;
  busy = true;
  error = null;
  try {
    await action();
  } catch (err) {
    error = text(err);
  } finally {
    busy = false;
  }
}

function open(): Promise<void> {
  const id = project?.id;
  if (!id) return Promise.resolve();
  return run(async () => {
    await load(id);
    reviewing = true;
  });
}

function trust(trusted: boolean): Promise<void> {
  const id = project?.id;
  const fingerprint = review?.current.fingerprint;
  if (!id) return Promise.resolve();
  return run(async () => {
    try {
      const updated = await transport.call({
        method: "projects.trust",
        params: trusted ? { id, trusted, fingerprint } : { id, trusted },
      });
      if (workspace.project?.id === id) workspace.project = updated.data;
      reviewing = false;
    } finally {
      // After a conflict this shows what changed meanwhile.
      await load(id);
    }
  });
}
</script>

{#if project}
  <section class="trust" class:changed aria-labelledby="trust-heading" data-testid="project-trust">
    <div class="row">
      <h3 id="trust-heading">{heading}</h3>
      {#if !reviewing && !changed}
        <button type="button" disabled={busy} onclick={open}>{project.trusted ? "Review" : "Review and trust…"}</button>
      {/if}
    </div>
    <p class="detail">
      {#if changed}
        Sessions run without the project's .claude/ settings until you trust what changed.
      {:else if project.trusted}
        Claude loads this project's .claude/ settings, hooks, agents, commands and skills.
      {:else}
        Claude reads the project's CLAUDE.md but not its .claude/ settings, hooks, agents, commands or skills.
      {/if}
      Changes apply to sessions started afterwards.
    </p>
    {#if changed && review}
      <div class="review" data-testid="trust-review">
        {#if diff.length === 0}
          <p class="detail">
            The summary is the same, but file contents changed (for example a hook script, a skill or an environment
            value).
          </p>
        {:else}
          {#if risky}
            <p class="warning" role="note">Some of what was added runs commands on your computer.</p>
          {/if}
          <ul class="items">
            {#each diff as change, n (n)}
              <li data-kind={change.kind}>
                <span class="kind">
                  {KIND_LABELS[change.kind]}
                  {#if runsCode(change.kind)}<strong>runs code</strong>{/if}
                </span>
                <span class="path">{change.path}</span>
                <ul>
                  {#each change.added as entry, m (m)}
                    <li class="added"><span class="sign" aria-label="Added">+</span> <code>{entry}</code></li>
                  {/each}
                  {#each change.removed as entry, m (m)}
                    <li class="removed"><span class="sign" aria-label="Removed">−</span> <code>{entry}</code></li>
                  {/each}
                </ul>
              </li>
            {/each}
          </ul>
        {/if}
        <div class="row actions">
          <button type="button" disabled={busy} onclick={() => trust(false)}>Stop trusting</button>
          <button type="button" class="primary" disabled={busy} onclick={() => trust(true)}>Trust the new settings</button>
        </div>
      </div>
    {:else if reviewing && review}
      <div class="review" data-testid="trust-review">
        {#if items.length === 0}
          <p class="detail">
            This project has no Claude settings of its own yet. If you trust it and settings appear later (for example
            after a git pull), Ovra asks again before they load.
          </p>
        {:else}
          <p class="detail">
            {project.trusted ? "Loaded" : "Trusting the project loads"} the following. If it changes later, Ovra asks again
            before loading it.
          </p>
          {#if risky}
            <p class="warning" role="note">Some of it runs commands on your computer.</p>
          {/if}
          <ul class="items">
            {#each items as item, n (n)}
              <li data-kind={item.kind}>
                <span class="kind">
                  {KIND_LABELS[item.kind]}
                  {#if runsCode(item.kind)}<strong>runs code</strong>{/if}
                </span>
                <span class="path">{item.path}</span>
                <ul>
                  {#each item.entries as entry, m (m)}
                    <li><code>{entry}</code></li>
                  {/each}
                </ul>
              </li>
            {/each}
          </ul>
        {/if}
        <div class="row actions">
          <button type="button" disabled={busy} onclick={() => (reviewing = false)}>Close</button>
          {#if project.trusted}
            <button type="button" disabled={busy} onclick={() => trust(false)}>Stop trusting</button>
          {:else}
            <button type="button" class="primary" disabled={busy} onclick={() => trust(true)}>Trust project</button>
          {/if}
        </div>
      </div>
    {:else if project.trusted}
      <button type="button" class="link" disabled={busy} onclick={() => trust(false)}>Stop trusting</button>
    {/if}
    {#if error}
      <p class="error" role="alert">{error}</p>
    {/if}
  </section>
{/if}

<style>
  .trust {
    margin: 0 12px 8px;
    padding: 6px 8px;
    border: 1px solid #2a2a31;
    border-radius: 3px;
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 12px;
  }
  h3 {
    margin: 0;
    font-size: 12px;
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  .actions {
    justify-content: flex-end;
  }
  p,
  ul {
    margin: 0;
  }
  .detail,
  .path {
    color: #b4b4c0;
  }
  .review {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .warning,
  strong {
    color: #ffd36b;
  }
  strong {
    margin-left: 4px;
    font-weight: 600;
  }
  .items {
    list-style: none;
    padding: 0;
    max-height: 30vh;
    overflow: auto;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .items > li {
    display: flex;
    flex-direction: column;
  }
  .items ul {
    padding-left: 14px;
  }
  code {
    font-family: ui-monospace, monospace;
    overflow-wrap: anywhere;
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
  button.primary {
    border-color: #7aa2ff;
  }
  button.link {
    align-self: flex-start;
    padding: 0;
    background: none;
    border: none;
    color: #9db8ff;
    text-decoration: underline;
  }
  button:disabled {
    opacity: 0.55;
    cursor: default;
  }
  button:focus-visible {
    outline: 2px solid #7aa2ff;
    outline-offset: 1px;
  }
  .error {
    color: #ff8f8f;
  }
  .trust.changed {
    border-color: #ffd36b;
  }
  .added .sign {
    color: #8fd18f;
  }
  .removed .sign {
    color: #ff8f8f;
  }
  .removed code {
    text-decoration: line-through;
  }
  @media (forced-colors: active) {
    .trust.changed {
      border: 2px solid Highlight;
    }
  }
</style>
