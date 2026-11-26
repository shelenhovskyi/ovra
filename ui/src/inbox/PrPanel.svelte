<script lang="ts">
// A published item's pull request: the chip that links to it, its checks
// with links, Refresh, Update when the worker has new reviewed work, and
// Send log for a failed check. Keep and Discard stay beside it in Reviews.
import type { PullRequest, ReviewItem } from "../lib/protocol";
import { useOvra } from "../shell/context";
import { CHECK_LABEL, canUpdatePr, isWebUrl, prChip, protectedWarning } from "./pull-request";
import type { ReviewInbox } from "./reviews.svelte";

let {
  inbox,
  item,
  pr,
  onsaid,
}: { inbox: ReviewInbox; item: ReviewItem; pr: PullRequest; onsaid: (text: string) => void } = $props();
const { transport } = useOvra();

const busy = $derived(inbox.busy.includes(item.id));
const checks = $derived(pr.status?.checks ?? []);
/** Files the review marks: an update pushes them only when the person ticks the box. */
const marked = $derived(item.files.filter((f) => f.protected).map((f) => f.path));
let includeProtected = $state(false);
/** The version the tick was given for: new files need a new one. */
let tickedFor = $state<string | null>(null);
const canPush = $derived(marked.length === 0 || (includeProtected && tickedFor === item.fingerprint));

/** Pages open in the system browser through the core, never in this window. */
function openPage(event: MouseEvent, url: string): void {
  event.preventDefault();
  transport.call({ method: "open.url", params: { url } }).catch((err: unknown) => {
    inbox.error = `Could not open ${url}: ${err instanceof Error ? err.message : String(err)}`;
  });
}

async function refresh(): Promise<void> {
  if (busy) return;
  if (await inbox.refreshPr(item.id)) onsaid("Pull request checked.");
}

async function update(): Promise<void> {
  if (busy || !canPush) return;
  if (await inbox.updatePr(item.id, marked.length > 0)) onsaid(`Pull request #${pr.number} updated.`);
}

async function sendLog(run: number, name: string): Promise<void> {
  if (busy) return;
  if (await inbox.sendLog(item.id, run))
    onsaid(`Sent the log of ${name} to the worker. This review updates when its changes do.`);
}
</script>

<div class="pr" role="group" aria-label="Pull request" data-testid="inbox-pr">
  <div class="head">
    {#if isWebUrl(pr.url)}
      <a class="chip" href={pr.url} data-testid="pr-chip" onclick={(e) => openPage(e, pr.url)}>{prChip(pr)}</a>
    {:else}
      <span class="chip" data-testid="pr-chip">{prChip(pr)}</span>
    {/if}
    <button type="button" class="quiet" aria-disabled={busy} onclick={refresh}>Refresh</button>
  </div>
  <p class="hint">
    <span class="mono">{pr.branch}</span> into <span class="mono">{pr.base}</span> on
    <span class="mono">{pr.remote}</span>
  </p>
  {#if checks.length > 0}
    <ul class="checks" aria-label="Checks">
      {#each checks as check, index (`${check.name}\n${index}`)}
        <li>
          <span class="state {check.state}">{CHECK_LABEL[check.state]}</span>
          {#if isWebUrl(check.url)}
            {@const url = check.url}
            <a href={url} onclick={(e) => openPage(e, url)}>{check.name}</a>
          {:else}
            <span>{check.name}</span>
          {/if}
          {#if check.state === "failed" && check.run !== null}
            {@const run = check.run}
            <button
              type="button"
              class="quiet"
              aria-disabled={busy}
              aria-label={`Send the log of ${check.name} to the worker`}
              onclick={() => sendLog(run, check.name)}>Send log to the worker</button
            >
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
  {#if canUpdatePr(item)}
    <p class="hint">The worker has new changes since the last push.</p>
    {#if marked.length > 0}
      <p class="hint" data-testid="pr-update-protected">{protectedWarning(marked)}</p>
      <label class="check">
        <input type="checkbox" data-testid="pr-update-include-protected" bind:checked={includeProtected}
          onchange={() => (tickedFor = item.fingerprint)}
        />
        Publish the protected files too
      </label>
    {/if}
    <div class="buttons">
      <button type="button" aria-disabled={busy || !canPush} onclick={update}>Update pull request</button>
    </div>
  {/if}
</div>

<style>
  .pr {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px 20px 0;
  }
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }
  p {
    margin: 0;
  }
  .hint {
    color: var(--text-2);
    font-size: 13px;
  }
  .mono {
    font-family: var(--mono);
  }
  .check {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
  }
  input[type="checkbox"] {
    accent-color: var(--accent);
  }
  .chip {
    padding: 2px 10px;
    font-size: 13px;
    font-weight: 500;
    color: var(--text);
    background: var(--s3);
    border-radius: var(--radius-xs);
    text-decoration: none;
  }
  a.chip:hover {
    background: var(--s4);
  }
  a {
    color: var(--accent);
  }
  a:focus-visible,
  button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
  .checks {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 0;
    padding: 0;
    list-style: none;
    font-size: 13px;
  }
  .checks li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    min-height: 24px;
  }
  .state {
    min-width: 56px;
    font-weight: 500;
  }
  .state.passed {
    color: var(--ok);
  }
  .state.failed {
    color: var(--danger);
  }
  .state.pending {
    color: var(--warn);
  }
  .state.skipped {
    color: var(--text-2);
  }
  .buttons {
    display: flex;
    gap: 8px;
  }
  button {
    min-height: var(--control-h-sm);
    padding: 5px 12px;
    font: inherit;
    font-weight: 500;
    color: var(--text);
    background: var(--s3);
    border: 0;
    border-radius: var(--radius);
    cursor: pointer;
  }
  button:hover {
    background: var(--s4);
  }
  button.quiet {
    color: var(--text-2);
    background: transparent;
  }
  button.quiet:hover {
    background: var(--s3);
  }
  button[aria-disabled="true"] {
    opacity: 0.55;
    cursor: default;
  }
  @media (forced-colors: active) {
    .chip {
      border: 1px solid CanvasText;
    }
    button[aria-disabled="true"] {
      color: GrayText;
    }
  }
</style>
