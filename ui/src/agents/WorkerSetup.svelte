<script lang="ts">
// A preparing worker's setup under its row (docs/specs/worktree-setup.md):
// the end of the current step's output on request, after a failed step
// the reason with Retry and Start anyway, and a note on copied files that
// look like secrets, which its agent can read.

import type { SessionId, SetupProgress } from "../lib/protocol";
import { CoreError } from "../lib/transport";
import { openSettings } from "../settings/commands";
import { useOvra } from "../shell/context";
import { agentSessions } from "./sessions.svelte";
import { stepLine } from "./worktrees";

const {
  session,
  setup,
  secrets = [],
}: { session: SessionId; setup: SetupProgress | null; secrets?: string[] } = $props();

const sessions = agentSessions(useOvra().transport);
let open = $state(false);
let busy = $state(false);
let error = $state<string | null>(null);
const failed = $derived(setup !== null && setup.failed !== null);
const logId = $derived(`setup-log-${session}`);

async function retry(skip: boolean): Promise<void> {
  busy = true;
  error = null;
  try {
    await sessions.retrySetup(session, skip);
  } catch (e) {
    error = e instanceof CoreError ? e.message : String(e);
  } finally {
    busy = false;
  }
}
</script>

<div class="setup" data-testid="worker-setup" role="group" aria-label="Worktree setup">
  {#if secrets.length > 0}
    <p class="note" data-testid="worker-setup-secrets">
      Copied into this worker's folder, where its agent can read them: {secrets.join(", ")}. For tokens, use
      <button type="button" class="link" onclick={() => openSettings("network")}>project secrets</button> instead.
    </p>
  {/if}
  {#if setup?.failed}
    <p class="why" data-testid="worker-setup-failed">Step {stepLine(setup)} {setup.failed}</p>
  {/if}
  <div class="actions">
    {#if setup && setup.log.length > 0}
      <button type="button" class="quiet" aria-expanded={open} aria-controls={logId} onclick={() => (open = !open)}
        >{open ? "Hide output" : "Show output"}</button
      >
    {/if}
    {#if failed}
      <span class="end">
        <button
          type="button"
          class="quiet"
          disabled={busy}
          title="Start the agent without a ready worktree"
          onclick={() => void retry(true)}>Start anyway</button
        >
        <button type="button" class="quiet strong" disabled={busy} onclick={() => void retry(false)}>Retry</button>
      </span>
    {/if}
  </div>
  {#if open && setup}
    <pre id={logId} class="log" data-testid="worker-setup-log">{setup.log.join("\n")}</pre>
  {/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
</div>

<style>
  .setup {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 0 8px 6px 28px;
  }
  .why {
    margin: 0;
    font-size: var(--fs-ui);
    color: var(--danger);
    overflow-wrap: anywhere;
  }
  .note {
    margin: 0;
    font-size: var(--fs-ui);
    color: var(--text-2);
    overflow-wrap: anywhere;
  }
  .link {
    min-height: 0;
    padding: 0;
    font: inherit;
    background: none;
    border: 0;
    color: var(--accent);
    text-decoration: underline;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .quiet {
    height: var(--control-h-sm);
    padding: 0 8px;
    font: inherit;
    font-size: var(--fs-ui);
    color: var(--text-2);
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--r-control);
  }
  .quiet:hover:not(:disabled) {
    color: var(--text);
    background: var(--s3);
  }
  .strong {
    color: var(--text);
    font-weight: 500;
  }
  .end {
    display: flex;
    gap: 4px;
    margin-left: auto;
  }
  .log {
    margin: 0;
    max-height: 160px;
    overflow: auto;
    padding: 6px 8px;
    border-radius: var(--radius-sm);
    background: var(--s3);
    font-family: var(--mono);
    font-size: var(--fs-ui-sm);
    color: var(--text-2);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .error {
    margin: 0;
    font-size: var(--fs-ui);
    color: var(--danger);
  }
</style>
