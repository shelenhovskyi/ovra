<script lang="ts">
// Project policies: a header button (and the "Project policies" command) opens
// a dialog with the rules that decide the open project's tool calls before
// the person is asked. Every add, edit or removal saves the whole list with
// `policy.set`; the core checks each rule. Escape closes, unless it closes an
// open list of suggestions or options first.
import { tick, untrack } from "svelte";
import { registerCommand } from "../keys/commands";
import type { PolicyAction, PolicyRule } from "../lib/protocol";
import { useOvra } from "../shell/context";
import { ChangedElsewhere, ProjectPolicy, reason } from "./policy.svelte";
import {
  ACTIONS,
  byPrecedence,
  caveats,
  formProblem,
  formRule,
  KNOWN_TOOLS,
  matchHint,
  ruleText,
  takesMatch,
  toolNote,
} from "./rules";

/** Whether to show the header button; the shell opens this from its "More" menu instead. */
let { trigger = true }: { trigger?: boolean } = $props();
const { transport, workspace } = useOvra();
const policy = new ProjectPolicy(transport);

let opened = $state(false);
let dialog = $state<HTMLElement>();
let toolField = $state<HTMLInputElement>();
let matchField = $state<HTMLInputElement>();
let openButton = $state<HTMLButtonElement>();
let returnTo: HTMLElement | null = null;

let tool = $state("");
let match = $state("");
let action = $state<PolicyAction>("ask");
/** The rule being edited: its place in the list and what it said then. */
let editing = $state<{ index: number; rule: PolicyRule } | null>(null);
let formError = $state<string | null>(null);
/** The field the error is about: the core refused the rule, or the form knows it would. */
let invalid = $state<"tool" | "match" | null>(null);
let busy = $state(false);
/**
 * The browser may show the Tool field's suggestions or the Action options:
 * Escape then closes those, not the dialog. A guess, since pages cannot ask.
 */
let listOpen = false;

const rows = $derived(byPrecedence(policy.rules ?? []));
const preview = $derived(formRule(tool, match, action));
const warning = $derived(toolNote(tool));
const toolHelp = $derived(
  [invalid === "tool" && formError ? "policy-error" : null, warning ? "policy-tool-warning" : null]
    .filter(Boolean)
    .join(" ") || undefined,
);
/** Turning the focused field off would drop focus out of the dialog. */
let matchFocused = $state(false);
/** A tool without a match: the field is off, unless it still holds text to clear. */
const matchOff = $derived(tool.trim() !== "" && !takesMatch(tool) && match.trim() === "" && !matchFocused);

$effect(() => () => policy.dispose());
$effect(() => registerCommand({ id: "policy.toggle", title: "Project policies", run: toggle }));

// Read the policy when the dialog opens, and again for another project.
// What was typed is cleared only for another project, not when the same
// project's details are refreshed.
let shownFor: string | null = null;
$effect(() => {
  const project = workspace.project?.id ?? null;
  if (!opened) {
    shownFor = null;
    return;
  }
  if (project === shownFor) return;
  shownFor = project;
  untrack(() => {
    clearForm();
    void policy.show(project);
  });
});

function same(a: PolicyRule | undefined, b: PolicyRule): boolean {
  return a !== undefined && a.tool === b.tool && a.match === b.match && a.action === b.action;
}

function clearForm(): void {
  tool = "";
  match = "";
  action = "ask";
  editing = null;
  formError = null;
  invalid = null;
  listOpen = false;
}

async function show(): Promise<void> {
  const active = document.activeElement;
  returnTo = active instanceof HTMLElement ? active : null;
  opened = true;
  await tick();
  dialog?.querySelector<HTMLElement>("h2")?.focus();
}

function close(): void {
  opened = false;
  clearForm();
  // Drops what was read, and any read still on its way.
  void policy.show(null);
  const back = returnTo?.isConnected && returnTo !== document.body ? returnTo : openButton;
  back?.focus();
  returnTo = null;
}

function toggle(): void {
  if (opened) close();
  else void show();
}

/**
 * Saves `rules`, built from `from`; on a refusal the form keeps what was typed.
 * Says whether it was kept, or whether the core refused the rules themselves.
 */
async function save(rules: PolicyRule[], from: PolicyRule[]): Promise<"kept" | "refused" | "not saved"> {
  // Buttons stay enabled while a call runs, so focus is never lost to the page.
  if (busy) return "not saved";
  busy = true;
  try {
    // With `from`, the core refuses a change built from an older policy, so a
    // rule added elsewhere meanwhile is never dropped; the list is read again.
    await policy.save(rules, from);
    formError = null;
    return "kept";
  } catch (err) {
    formError = plain(reason(err), rules);
    return err instanceof ChangedElsewhere ? "not saved" : "refused";
  } finally {
    busy = false;
  }
}

/** The core's refusal without the `Tool(match): ` it starts with, as a sentence. */
function plain(text: string, rules: PolicyRule[]): string {
  const rule = rules.find((r) => text.startsWith(`${ruleText(r)}: `));
  const rest = rule ? text.slice(ruleText(rule).length + 2) : text;
  return `${rest.charAt(0).toUpperCase()}${rest.slice(1)}`;
}

async function submit(event: SubmitEvent): Promise<void> {
  event.preventDefault();
  const current = policy.rules;
  if (!current) return;
  const problem = formProblem(preview);
  if (problem) {
    // Never sent: the core would refuse it, and one message is enough.
    formError = problem.text;
    invalid = problem.field;
    await tick();
    (invalid === "match" ? matchField : toolField)?.focus();
    return;
  }
  const target = editing;
  if (target && !same(current[target.index], target.rule)) {
    // The rule changed elsewhere while it was in the form; what was typed
    // stays, as a new rule, so nothing is overwritten and nothing is lost.
    editing = null;
    invalid = null;
    formError =
      "The rule you were editing changed elsewhere. What you typed is kept as a new rule: check the list, then add it.";
    toolField?.focus();
    return;
  }
  const next = target ? current.map((rule, i) => (i === target.index ? preview : rule)) : [...current, preview];
  const saved = await save(next, current);
  invalid = saved === "refused" ? "tool" : null;
  if (saved === "kept") {
    clearForm();
    toolField?.focus();
  }
}

async function edit(index: number, rule: PolicyRule): Promise<void> {
  editing = { index, rule };
  tool = rule.tool;
  match = rule.match ?? "";
  action = rule.action;
  formError = null;
  invalid = null;
  await tick();
  toolField?.focus();
}

function cancel(): void {
  clearForm();
  toolField?.focus();
}

async function remove(index: number): Promise<void> {
  const current = policy.rules;
  if (!current) return;
  const next = current.filter((_, i) => i !== index);
  if ((await save(next, current)) === "kept") {
    // Places after the removed rule move up; an edit of the removed rule
    // keeps what was typed, as a new rule.
    const at = editing;
    if (at?.index === index) editing = null;
    else if (at && at.index > index) editing = { ...at, index: at.index - 1 };
    await tick();
    // The removed row's button is gone; keep focus inside the dialog.
    if (!dialog?.contains(document.activeElement)) toolField?.focus();
  }
}

function onToolInput(event: Event): void {
  const value = (event.currentTarget as HTMLInputElement).value;
  const typed = value.trim().toLowerCase();
  listOpen = typed !== "" && KNOWN_TOOLS.some((name) => name !== value && name.toLowerCase().startsWith(typed));
}

function onListKeydown(event: KeyboardEvent): void {
  const arrow = event.code === "ArrowDown" || event.code === "ArrowUp";
  const opens =
    event.currentTarget instanceof HTMLSelectElement
      ? event.code === "Space" || event.code === "Enter" || (event.altKey && arrow)
      : arrow;
  if (opens) listOpen = true;
}

function onDialogKeydown(event: KeyboardEvent): void {
  // Shortcuts of the panels behind the modal must not act.
  event.stopPropagation();
  if (event.code === "Escape") {
    if (listOpen) {
      // The browser closes its list; the dialog stays.
      listOpen = false;
      return;
    }
    event.preventDefault();
    close();
    return;
  }
  if (event.code !== "Tab") return;
  // Keep Tab inside the dialog (aria-modal), also from the heading.
  const focusable = [
    ...(dialog?.querySelectorAll<HTMLElement>("button, input, select, summary, [tabindex]") ?? []),
  ].filter((el) => el.tabIndex >= 0 && !el.matches(":disabled"));
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

{#if trigger}
  <button
    type="button"
    class="open-policy"
    data-testid="open-policy"
    bind:this={openButton} aria-haspopup="dialog" onclick={() => void show()}>
    Policies
  </button>
{/if}

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
          <p class="muted intro" data-testid="policy-intro">
            Rules decide what agents in this project may do without you: Deny refuses a tool, Ask asks you first, Auto
            allows it without asking. Anything no rule covers is asked about.
          </p>
          <details class="muted">
            <summary>How rules combine</summary>
            <p>
              Deny rules are checked first, then Ask, then Auto: the first that matches decides. Auto works only for an
              agent in the sandbox with limited network access. Your own Claude settings still apply: Deny and Ask here
              override them, and Auto never allows a call they refuse.
            </p>
          </details>
          {#if policy.error}
            <p class="error" role="alert">Could not read the project's policy: {policy.error}</p>
            <button type="button" data-testid="policy-retry" onclick={() => void policy.reload()}>
              {policy.loading ? "Loading…" : "Try again"}
            </button>
          {/if}
          {#if rows.length === 0 && !policy.error}
            <p class="muted" data-testid="policy-empty">
              {policy.rules === null ? "Loading…" : "No rules yet: what your Claude settings do not allow is asked about."}
            </p>
          {:else if rows.length > 0}
            <ol aria-label="Rules, in the order they are checked">
              {#each rows as { rule, index } (index)}
                {@const notes = caveats(rule)}
                {@const about = notes.length ? `policy-notes-${index}` : undefined}
                <li data-testid="policy-rule" class={rule.action}>
                  <div class="line">
                    <code>{ruleText(rule)}</code>
                    <span class="action">{ACTIONS[rule.action]}</span>
                    <button type="button" aria-label={`Edit ${ruleText(rule)} ${rule.action}`} aria-describedby={about} onclick={() => void edit(index, rule)}>Edit</button>
                    <button type="button" aria-label={`Remove ${ruleText(rule)} ${rule.action}`} aria-describedby={about} onclick={() => void remove(index)}>Remove</button>
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
          <form onsubmit={submit} aria-label={editing ? "Edit rule" : "Add a rule"}>
            <!-- The core replaces the whole list, so changes wait until it is known. -->
            <fieldset disabled={policy.rules === null}>
              <label>
                Tool
                <input
                  bind:this={toolField}
                  bind:value={tool}
                  list="policy-tools"
                  placeholder="Choose or type a tool"
                  autocomplete="off"
                  spellcheck="false"
                  aria-invalid={invalid === "tool" ? "true" : undefined}
                  aria-describedby={toolHelp}
                  oninput={onToolInput}
                  onkeydown={onListKeydown}
                  onblur={() => (listOpen = false)}
                  onchange={() => (listOpen = false)}
                  data-testid="policy-tool"
                />
              </label>
              <datalist id="policy-tools">
                {#each KNOWN_TOOLS as name (name)}
                  <option value={name}></option>
                {/each}
              </datalist>
              <label>
                Match (optional)
                <input
                  bind:this={matchField}
                  bind:value={match}
                  placeholder={matchOff ? "Covers every call" : "For example npm test or docs/**"}
                  disabled={matchOff}
                  autocomplete="off"
                  spellcheck="false"
                  aria-invalid={invalid === "match" ? "true" : undefined}
                  aria-describedby={invalid === "match" && formError ? "policy-error" : "policy-match-hint"}
                  onfocus={() => (matchFocused = true)}
                  onblur={() => (matchFocused = false)}
                  data-testid="policy-match"
                />
              </label>
              <label>
                Action
                <select
                  bind:value={action}
                  onpointerdown={() => (listOpen = !listOpen)}
                  onkeydown={onListKeydown}
                  onblur={() => (listOpen = false)}
                  onchange={() => (listOpen = false)}
                  data-testid="policy-action"
                >
                  <option value="deny">Deny</option>
                  <option value="ask">Ask</option>
                  <option value="auto">Auto</option>
                </select>
              </label>
              <button type="submit">{editing ? "Save rule" : "Add rule"}</button>
              {#if editing}
                <button type="button" onclick={cancel}>Cancel</button>
              {/if}
            </fieldset>
          </form>
          {#if warning}
            <p class="warning" id="policy-tool-warning" data-testid="policy-tool-warning">{warning}</p>
          {/if}
          {#if !(invalid === "match" && formError)}
            <p class="muted" id="policy-match-hint">{matchHint(tool)}</p>
          {/if}
          {#if formError}
            <p class="error" role="alert" data-testid="policy-error">{formError}</p>
          {/if}
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  button,
  input,
  select {
    font: inherit;
    padding: 2px 10px;
    color: #e6e6e6;
    background: #2a2a33;
    border: 1px solid #4a4a57;
    border-radius: 3px;
  }
  button {
    cursor: pointer;
  }
  input::placeholder {
    color: #9a9aa8;
    font-style: italic;
  }
  input:disabled {
    cursor: not-allowed;
    opacity: 0.55;
  }
  details p {
    margin: 4px 0 0;
  }
  button:focus-visible,
  input:focus-visible,
  select:focus-visible,
  summary:focus-visible,
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
  .warning {
    color: #ffd36b;
    font-size: 12px;
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
  .line button:first-of-type {
    margin-left: auto;
  }
  .notes {
    margin: 4px 0 0;
    padding-left: 16px;
    color: #ffd36b;
    font-size: 12px;
  }
  form {
    display: flex;
    flex-wrap: wrap;
    align-items: end;
    gap: 8px;
    margin: 10px 0 4px;
  }
  fieldset {
    display: contents;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: 12px;
    color: #c8c8d2;
  }
</style>
