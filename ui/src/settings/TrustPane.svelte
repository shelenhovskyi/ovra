<script lang="ts">
// Settings > Trust: whether the open project's own Claude settings load,
// and the full list of what they bring (hooks, rules, skills, ...). It only
// reads; trusting stays on the trust card in the Agents column ("Review and
// trust…"), where Trust sits under the same list, armed after a moment.
import { untrack } from "svelte";
import { changes, groups, KIND_LABELS, runsCode, summary } from "../agents/setup/texts";
import { listCommands } from "../keys/commands";
import type { ProjectConfigReview } from "../lib/protocol";
import { CoreError } from "../lib/transport";
import { useOvra } from "../shell/context";

const { transport, workspace } = useOvra();

let review = $state<ProjectConfigReview | null>(null);
let error = $state<string | null>(null);

const project = $derived(workspace.project);
const items = $derived(review?.current.items ?? []);
const changed = $derived(
  project?.trusted === true && review !== null && review.trusted?.fingerprint !== review.current.fingerprint,
);
const standing = $derived(
  !review
    ? null
    : changed
      ? "Changed since you trusted them: Claude runs without them until you trust the changes."
      : project?.trusted
        ? "Trusted: Claude uses them. If they change, Ovra asks again."
        : items.length === 0
          ? "None: this project brings no Claude settings of its own."
          : "Not trusted: Claude runs with your own settings only.",
);
const listed = $derived(
  groups(
    changed && review?.trusted
      ? changes(review.trusted, review.current).map((c) => ({
          path: c.path,
          kind: c.kind,
          rows: [
            ...c.added.map((text) => ({ text: `+ ${text}` })),
            ...c.removed.map((text) => ({ text: `− ${text}` })),
          ],
        }))
      : items.map((item) => ({ path: item.path, kind: item.kind, rows: item.entries.map((text) => ({ text })) })),
  ),
);

$effect(() => {
  const id = project?.id ?? null;
  void project?.trusted;
  untrack(() => {
    if (!id) return;
    transport
      .call({ method: "agents.project_config", params: { project: id } })
      .then((answer) => {
        if (workspace.project?.id === id) review = answer.data;
        error = null;
      })
      .catch((err: unknown) => {
        error = err instanceof CoreError ? err.error.message : String(err);
      });
  });
});

function reviewAndTrust(): void {
  void listCommands()
    .find((c) => c.id === "projects.review_trust")
    ?.run();
}
</script>

<div class="trust" data-testid="trust-settings">
  {#if error}
    <p class="error" role="alert">Could not read the project's Claude settings: {error}</p>
  {:else if !review}
    <p class="muted">Reading the project's settings…</p>
  {:else}
    <p class:warn={changed} data-testid="trust-state">{standing}</p>
    {#if items.length > 0}
      <p class="muted">{summary(items)}</p>
      {#if !project?.trusted || changed}
        <button type="button" onclick={reviewAndTrust}>Review and trust…</button>
      {/if}
      {#each listed as group (group.kind)}
        <section aria-label={KIND_LABELS[group.kind]}>
          <h4 class:warn={runsCode(group.kind) || group.kind === "broad_allow_rules"}>{KIND_LABELS[group.kind]}</h4>
          {#each group.files as file (file.path)}
            <p class="file"><code>{file.path}</code></p>
            <ul>
              {#each file.rows as row, i (i)}
                <li><code>{row.text}</code></li>
              {/each}
            </ul>
          {/each}
        </section>
      {/each}
    {/if}
  {/if}
</div>

<style>
  .trust {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    max-width: 560px;
  }
  p {
    margin: 0;
  }
  .muted {
    font: var(--t-small);
    color: var(--text-2);
  }
  .warn {
    color: var(--warn);
  }
  .error {
    color: var(--danger);
  }
  h4 {
    margin: var(--sp-3) 0 var(--sp-1);
    font: var(--t-headline);
  }
  .file {
    font: var(--t-small);
    color: var(--text-2);
  }
  ul {
    margin: 2px 0 var(--sp-2);
    padding-left: var(--sp-4);
  }
  code {
    font: 13px / 18px var(--mono);
    overflow-wrap: anywhere;
  }
  button {
    align-self: flex-start;
    height: var(--ctl);
    padding: 0 var(--sp-3);
    font: inherit;
    color: var(--text);
    background: var(--s3);
    border: 0;
    border-radius: var(--r-md);
    cursor: default;
  }
  button:hover {
    background: var(--s4);
  }
</style>
