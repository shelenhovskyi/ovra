<script lang="ts">
// Worktree folders no task knows (a crash, an older Ovra), with their sizes
// (docs/specs/worktree-setup.md). Only the person removes one, after a
// confirmation; Ovra never does on its own.

import { tick } from "svelte";
import type { ProjectId } from "../lib/protocol";
import { CoreError } from "../lib/transport";
import { useOvra } from "../shell/context";
import { diskLabel } from "./resources";
import { agentSessions } from "./sessions.svelte";
import { folderName } from "./worktrees";

const { project }: { project: ProjectId } = $props();

const sessions = agentSessions(useOvra().transport);
const list = $derived(sessions.leftovers.get(project) ?? []);
/** The folder whose Remove asks for confirmation. */
let asking = $state<string | null>(null);
let busy = $state(false);
let error = $state<string | null>(null);

async function ask(path: string): Promise<void> {
  asking = path;
  error = null;
  await tick();
  document.querySelector<HTMLButtonElement>("[data-testid=leftover-confirm] .danger")?.focus();
}

async function remove(path: string): Promise<void> {
  busy = true;
  try {
    await sessions.removeLeftover(project, path);
    asking = null;
  } catch (e) {
    error = e instanceof CoreError ? e.message : String(e);
  } finally {
    busy = false;
  }
}

function onkeydown(event: KeyboardEvent): void {
  if (event.code !== "Escape") return;
  event.preventDefault();
  event.stopPropagation();
  asking = null;
}
</script>

{#if list.length > 0}
  <section class="leftovers" aria-labelledby="leftovers-heading" data-testid="leftovers">
    <h3 id="leftovers-heading">Leftover worktrees</h3>
    <p class="note">Folders no task uses. Ovra keeps them until you remove them.</p>
    <ul>
      {#each list as item (item.path)}
        <li>
          <span class="name" title={item.path}>{folderName(item.path)}</span>
          <span class="size">{item.disk ? diskLabel(item.disk) : "size unknown"}</span>
          <button
            type="button"
            class="quiet"
            aria-label={`Remove ${folderName(item.path)}`}
            disabled={busy}
            onclick={() => void ask(item.path)}>Remove</button
          >
        </li>
        {#if asking === item.path}
          <li>
            <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
            <div
              class="confirm"
              role="group"
              aria-label="Remove this folder?"
              data-testid="leftover-confirm"
              {onkeydown}
            >
              <p>Delete {folderName(item.path)} and everything in it? This cannot be undone.</p>
              <span class="buttons">
                <button type="button" class="quiet" disabled={busy} onclick={() => (asking = null)}>Cancel</button>
                <button type="button" class="danger" disabled={busy} onclick={() => void remove(item.path)}
                  >Delete</button
                >
              </span>
            </div>
          </li>
        {/if}
      {/each}
    </ul>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
  </section>
{/if}

<style>
  .leftovers {
    margin: 12px 8px 0;
  }
  h3 {
    margin: 0;
    font-size: 13px;
    font-weight: 600;
    color: var(--text-2);
  }
  .note {
    margin: 2px 0 4px;
    font-size: 13px;
    color: var(--text-2);
  }
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  li {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 28px;
  }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--mono);
    font-size: 13px;
  }
  .size {
    font-size: 13px;
    color: var(--text-2);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .confirm {
    flex: 1;
    padding: 8px;
    border-radius: var(--radius-sm);
    background: var(--s3);
  }
  .confirm p {
    margin: 0 0 6px;
    font-size: 13px;
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 4px;
  }
  .quiet {
    height: var(--control-h-sm);
    padding: 0 8px;
    font: inherit;
    font-size: 13px;
    color: var(--text-2);
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
  }
  .quiet:hover:not(:disabled) {
    color: var(--text);
    background: var(--s3);
  }
  .danger {
    height: var(--control-h-sm);
    padding: 0 10px;
    font-size: 13px;
    font-weight: 500;
    color: var(--danger);
    background: var(--danger-soft);
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
  }
  .error {
    margin: 4px 0 0;
    font-size: 13px;
    color: var(--danger);
  }
</style>
