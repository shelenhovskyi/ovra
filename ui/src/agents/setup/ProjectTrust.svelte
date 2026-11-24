<script lang="ts">
// Per-project trust. Until the user trusts a project, Claude runs with the
// user's own settings only; trusting it also loads the project's .claude/
// settings, hooks, agents, commands and skills. Hooks and helpers run
// commands, so the card shows everything that would load before asking.
import { untrack } from "svelte";
import type { ProjectAgentConfig } from "../../lib/protocol";
import { CoreError } from "../../lib/transport";
import { useOvra } from "../../shell/context";
import { KIND_LABELS, runsCode } from "./texts";

const { transport, workspace } = useOvra();

let config = $state<ProjectAgentConfig | null>(null);
let reviewing = $state(false);
let busy = $state(false);
let error = $state<string | null>(null);

const project = $derived(workspace.project);
const risky = $derived(config?.items.some((item) => runsCode(item.kind)) ?? false);
const items = $derived([...(config?.items ?? [])].sort((a, b) => Number(runsCode(b.kind)) - Number(runsCode(a.kind))));

$effect(() => {
  void project?.id;
  untrack(() => {
    reviewing = false;
    config = null;
    error = null;
  });
});

function text(err: unknown): string {
  if (err instanceof CoreError) return err.error.message;
  return err instanceof Error ? err.message : String(err);
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

function review(): Promise<void> {
  const id = project?.id;
  if (!id) return Promise.resolve();
  return run(async () => {
    config = (await transport.call({ method: "agents.project_config", params: { project: id } })).data;
    reviewing = true;
  });
}

function trust(trusted: boolean): Promise<void> {
  const id = project?.id;
  if (!id) return Promise.resolve();
  return run(async () => {
    const updated = await transport.call({ method: "projects.trust", params: { id, trusted } });
    if (workspace.project?.id === id) workspace.project = updated.data;
    reviewing = false;
  });
}
</script>

{#if project}
  <section class="trust" aria-labelledby="trust-heading" data-testid="project-trust">
    <div class="row">
      <h3 id="trust-heading">{project.trusted ? "Project settings: trusted" : "Project settings: not loaded"}</h3>
      {#if !reviewing}
        <button type="button" disabled={busy} onclick={review}>{project.trusted ? "Review" : "Review and trust…"}</button>
      {/if}
    </div>
    <p class="detail">
      {#if project.trusted}
        Claude loads this project's .claude/ settings, hooks, agents, commands and skills.
      {:else}
        Claude reads the project's CLAUDE.md but not its .claude/ settings, hooks, agents, commands or skills.
      {/if}
      Changes apply to sessions started afterwards.
    </p>
    {#if reviewing && config}
      <div class="review" data-testid="trust-review">
        {#if items.length === 0}
          <p class="detail">
            This project has no Claude settings of its own yet. If you trust it, settings added later (for example by a
            git pull) load without asking again.
          </p>
        {:else}
          <p class="detail">
            {project.trusted ? "Loaded" : "Trusting the project loads"} the following. Settings changed later (for example
            by a git pull) load without asking again.
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
</style>
