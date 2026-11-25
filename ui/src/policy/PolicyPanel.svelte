<script lang="ts">
// Project policies: a header button (and the "Project policies" command) opens
// a dialog with the rules that decide the open project's tool calls before
// the person is asked, in the order the core checks them. Escape closes.
import { tick, untrack } from "svelte";
import { registerCommand } from "../keys/commands";
import { useOvra } from "../shell/context";
import { ProjectPolicy } from "./policy.svelte";
import { ACTIONS, byPrecedence, caveats, ruleText } from "./rules";

const { transport, workspace } = useOvra();
const policy = new ProjectPolicy(transport);

let opened = $state(false);
let dialog = $state<HTMLElement>();
let returnTo: HTMLElement | null = null;

const rows = $derived(byPrecedence(policy.rules ?? []));

$effect(() => () => policy.dispose());
$effect(() => registerCommand({ id: "policy.toggle", title: "Project policies", run: toggle }));

// Read the policy when the dialog opens, and again for another project.
$effect(() => {
  const project = workspace.project?.id ?? null;
  if (!opened) return;
  untrack(() => void policy.show(project));
});

async function show(): Promise<void> {
  const active = document.activeElement;
  returnTo = active instanceof HTMLElement ? active : null;
  opened = true;
  await tick();
  dialog?.querySelector<HTMLElement>("h2")?.focus();
}

function close(): void {
  opened = false;
  if (returnTo?.isConnected) returnTo.focus();
  returnTo = null;
}

function toggle(): void {
  if (opened) close();
  else void show();
}

function onDialogKeydown(event: KeyboardEvent): void {
  // Shortcuts of the panels behind the modal must not act.
  event.stopPropagation();
  if (event.code === "Escape") {
    event.preventDefault();
    close();
    return;
  }
  if (event.code !== "Tab") return;
  // Keep Tab inside the dialog (aria-modal), also from the heading.
  const focusable = [...(dialog?.querySelectorAll<HTMLElement>("button, [tabindex]") ?? [])].filter(
    (el) => el.tabIndex >= 0 && !el.matches(":disabled"),
  );
  const first = focusable[0];
  const last = focusable[focusable.length - 1];
  if (!first || !last) return;
  const at = focusable.indexOf(document.activeElement as HTMLElement);
  if (at < 0) {
    event.preventDefault();
    (event.shiftKey ? last : first).focus();
  } else if (event.shiftKey && at === 0) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && at === focusable.length - 1) {
    event.preventDefault();
    first.focus();
  }
}
</script>

<button type="button" class="open-policy" data-testid="open-policy" aria-haspopup="dialog" onclick={() => void show()}>
  Policies
</button>

{#if opened}
  <!-- A click beside the dialog must not take focus out of it. -->
  <div class="backdrop" role="presentation" onmousedown={(event) => event.target === event.currentTarget && event.preventDefault()}>
    <div
      class="policy"
      role="dialog"
      aria-modal="true"
      aria-labelledby="policy-title"
      tabindex="-1"
      data-testid="policy"
      bind:this={dialog}
      onkeydown={onDialogKeydown}
    >
      <header>
        <h2 id="policy-title" tabindex="-1">Project policies</h2>
        <span class="muted">{workspace.project?.name ?? "No project"}</span>
        <button type="button" class="close" aria-keyshortcuts="Escape" onclick={close}>Close</button>
      </header>
      {#if !workspace.project}
        <p class="muted note">Open a project to see its policies.</p>
      {:else}
        <div class="body">
          <p class="muted">
            Rules decide an agent's tool call before you are asked. Deny rules are checked first, then ask, then auto;
            a call no rule matches is asked about.
          </p>
          {#if policy.error}
            <p class="error" role="alert">Could not read the project's policy: {policy.error}</p>
          {/if}
          {#if rows.length === 0 && !policy.error}
            <p class="muted" data-testid="policy-empty">
              {policy.rules === null ? "Loading…" : "No rules yet: every call is asked about."}
            </p>
          {:else if rows.length > 0}
            <ol aria-label="Rules, in the order they are checked">
              {#each rows as { rule, index } (index)}
                {@const notes = caveats(rule)}
                <li data-testid="policy-rule" class={rule.action} aria-describedby={notes.length ? `policy-notes-${index}` : undefined}>
                  <div class="line">
                    <code>{ruleText(rule)}</code>
                    <span class="action">{ACTIONS[rule.action]}</span>
                  </div>
                  {#if notes.length > 0}
                    <ul class="notes" id={`policy-notes-${index}`} data-testid="policy-caveat">
                      {#each notes as note (note)}
                        <li>{note}</li>
                      {/each}
                    </ul>
                  {/if}
                </li>
              {/each}
            </ol>
          {/if}
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  button {
    font: inherit;
    padding: 2px 10px;
    color: #e6e6e6;
    background: #2a2a33;
    border: 1px solid #4a4a57;
    border-radius: 3px;
    cursor: pointer;
  }
  button:focus-visible,
  h2:focus-visible {
    outline: 2px solid #7aa2ff;
    outline-offset: 1px;
  }
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 90;
    display: flex;
    justify-content: center;
    padding: 24px;
    background: rgb(0 0 0 / 55%);
  }
  .policy {
    flex: 1;
    max-width: 760px;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    background: #16161a;
    border: 1px solid #3a3a44;
    border-radius: 6px;
  }
  header {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 12px;
    border-bottom: 1px solid #2a2a31;
  }
  .close {
    margin-left: auto;
  }
  h2 {
    margin: 0;
    font-size: 14px;
  }
  .body {
    flex: 1;
    overflow: auto;
    padding: 0 12px 12px;
  }
  .muted {
    color: #b4b4c0;
    font-size: 12px;
  }
  .note {
    margin: 6px 12px;
  }
  .error {
    color: #ff8f8f;
    margin: 6px 0;
  }
  ol {
    list-style: none;
    margin: 6px 0;
    padding: 0;
  }
  ol > li {
    padding: 6px 8px;
    border-left: 3px solid #3a3a44;
    border-bottom: 1px solid #24242b;
  }
  ol > li.deny {
    border-left-color: #ff8f8f;
  }
  ol > li.auto {
    border-left-color: #ffd36b;
  }
  .line {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 8px;
  }
  .action {
    color: #c8c8d2;
  }
.notes {
    margin: 4px 0 0;
    padding-left: 16px;
    color: #ffd36b;
    font-size: 12px;
  }
</style>
