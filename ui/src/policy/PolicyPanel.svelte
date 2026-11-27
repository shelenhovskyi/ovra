<script lang="ts">
// Project policies, the Rules pane of Settings: the rules that decide the
// open project's tool calls before the person is asked. Every add, edit or
// removal saves the whole list with `policy.set`; the core checks each rule.
// Escape that closes an open list of suggestions or options stays here; the
// next one closes Settings.
import { tick, untrack } from "svelte";
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
  ruleLabel,
  ruleText,
  takesExact,
  takesMatch,
  toolNote,
} from "./rules";

const { transport, workspace } = useOvra();
const policy = new ProjectPolicy(transport);

let pane = $state<HTMLElement>();
let toolField = $state<HTMLInputElement>();
let matchField = $state<HTMLInputElement>();

let tool = $state("");
let match = $state("");
let action = $state<PolicyAction>("ask");
/** A Bash auto match is the whole command, not a prefix. */
let exact = $state(false);
/** The rule being edited: its place in the list and what it said then. */
let editing = $state<{ index: number; rule: PolicyRule } | null>(null);
let formError = $state<string | null>(null);
/** The field the error is about: the core refused the rule, or the form knows it would. */
let invalid = $state<"tool" | "match" | null>(null);
let busy = $state(false);
/**
 * The browser may show the Tool field's suggestions or the Action options:
 * Escape then closes those, not Settings. A guess, since pages cannot ask.
 */
let listOpen = false;

const rows = $derived(byPrecedence(policy.rules ?? []));
const preview = $derived(formRule(tool, match, action, exact));
const warning = $derived(toolNote(tool));
const toolHelp = $derived(
  [invalid === "tool" && formError ? "policy-error" : null, warning ? "policy-tool-warning" : null]
    .filter(Boolean)
    .join(" ") || undefined,
);
/** Turning the focused field off would drop focus out of the pane. */
let matchFocused = $state(false);
/** A tool without a match: the field is off, unless it still holds text to clear. */
const matchOff = $derived(tool.trim() !== "" && !takesMatch(tool) && match.trim() === "" && !matchFocused);

$effect(() => () => policy.dispose());

// Read the policy, and again for another project. What was typed is
// cleared only for another project, not when the same project's details
// are refreshed.
let shownFor: string | null | undefined;
$effect(() => {
  const project = workspace.project?.id ?? null;
  if (project === shownFor) return;
  shownFor = project;
  untrack(() => {
    clearForm();
    void policy.show(project);
  });
});

function same(a: PolicyRule | undefined, b: PolicyRule): boolean {
  return (
    a !== undefined && a.tool === b.tool && a.match === b.match && a.action === b.action && !!a.exact === !!b.exact
  );
}

function clearForm(): void {
  tool = "";
  match = "";
  action = "ask";
  exact = false;
  editing = null;
  formError = null;
  invalid = null;
  listOpen = false;
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

let quietError = $state<string | null>(null);

async function toggleQuiet(event: Event): Promise<void> {
  const box = event.currentTarget as HTMLInputElement;
  const on = box.checked;
  if (busy) {
    box.checked = policy.quiet;
    return;
  }
  busy = true;
  try {
    await policy.setQuiet(on);
    quietError = null;
  } catch (err) {
    quietError = reason(err);
  } finally {
    busy = false;
    // What the core keeps, also after a refusal.
    box.checked = policy.quiet;
  }
}

async function edit(index: number, rule: PolicyRule): Promise<void> {
  editing = { index, rule };
  tool = rule.tool;
  match = rule.match ?? "";
  action = rule.action;
  exact = rule.exact ?? false;
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
    // The removed row's button is gone; keep focus inside the pane.
    if (!pane?.contains(document.activeElement)) toolField?.focus();
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

/** Escape closes the browser's open list first, and goes no further: Settings stays open. */
function onPaneKeydown(event: KeyboardEvent): void {
  if (event.code !== "Escape" || !listOpen) return;
  listOpen = false;
  event.stopPropagation();
}
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="policy" data-testid="policy" bind:this={pane} onkeydown={onPaneKeydown}>
  {#if workspace.project}
    <div class="body">
      <details class="muted">
        <summary>How rules combine</summary>
        <p>
          Deny rules are checked first, then Ask, then Auto: the first that matches decides. Auto works only for an
          agent in the sandbox with limited network access. Your own Claude settings still apply: Deny and Ask here
          override them, and Auto never allows a call they refuse.
        </p>
      </details>
      <section class="quiet" aria-labelledby="policy-quiet-title">
        <label class="check">
          <input
            type="checkbox"
            checked={policy.quiet}
            disabled={policy.rules === null}
            aria-describedby="policy-quiet-about"
            onchange={(event) => void toggleQuiet(event)}
            data-testid="policy-quiet"
          />
          <span id="policy-quiet-title">Don't ask about what the sandbox contains</span>
        </label>
        <p class="muted" id="policy-quiet-about">
          Reads in the project folder and web requests to named sites run without asking, and so do edits and commands
          of agents working in their own copy. Protected files, git changes, files your rules hold, work in your own
          folder, keys and pushes still ask. Works only where Ovra's sandbox is verified (Linux for now); elsewhere
          agents ask as before.
        </p>
        {#if policy.quiet}
          <p class="warning" data-testid="policy-quiet-risk">
            A harmful instruction in a file could run without you seeing it, and could send data to any site your
            network allowlist lets through. Its changes still wait for your review.
          </p>
        {/if}
        {#if quietError}
          <p class="error" role="alert" data-testid="policy-quiet-error">{quietError}</p>
        {/if}
      </section>
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
                {#if rule.exact}<span class="origin" data-testid="policy-exact">exactly</span>{/if}
                <span class="action">{ACTIONS[rule.action]}</span>
                {#if rule.from_approval}<span class="origin" data-testid="policy-from-approval">From an approval</span>{/if}
                <button type="button" aria-label={`Edit ${ruleLabel(rule)}`} aria-describedby={about} onclick={() => void edit(index, rule)}>Edit</button>
                <button type="button" aria-label={`Remove ${ruleLabel(rule)}`} aria-describedby={about} onclick={() => void remove(index)}>Remove</button>
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
          {#if takesExact(tool, action)}
            <label class="check">
              <input type="checkbox" bind:checked={exact} data-testid="policy-exact-field" />
              Exact command
            </label>
          {/if}
          <label>
            Action
            <select
              bind:value={action}
              onpointerdown={() => (listOpen = !listOpen)}
              onkeydown={onListKeydown}
              onblur={() => (listOpen = false)}
              onchange={() => {
                listOpen = false;
                // Exact is for Auto only; another action starts without it.
                exact = false;
              }}
              data-testid="policy-action"
            >
              <option value="deny">Deny</option>
              <option value="ask">Ask</option>
              <option value="auto">Auto</option>
            </select>
          </label>
          <button type="submit" class="primary">{editing ? "Save rule" : "Add rule"}</button>
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

<style>
  .muted,
  label {
    color: var(--text-2);
    font-size: 13px;
  }
  button,
  input,
  select {
    font: inherit;
    padding: 5px 10px;
    color: var(--text);
    background: var(--s3);
    border: 0;
    border-radius: var(--radius);
  }
  button {
    font-weight: 500;
    cursor: pointer;
  }
  button:hover {
    color: var(--text);
    background: var(--s4);
  }
  button.primary {
    color: var(--accent-ink);
    background: var(--accent);
  }
  input:disabled {
    cursor: not-allowed;
    opacity: 0.55;
  }
  summary {
    cursor: pointer;
  }
  details p {
    margin: 4px 0 0;
  }
  .error {
    color: var(--danger);
    margin: 6px 0;
  }
  .warning {
    color: var(--warn);
    font-size: 13px;
    margin: 6px 0;
  }
  ol {
    list-style: none;
    display: grid;
    gap: 4px;
    margin: 12px 0;
    padding: 0;
  }
  ol > li {
    padding: 8px 10px;
    border-radius: var(--radius);
    background: var(--s1);
  }
  .line {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }
  .action {
    padding: 1px 8px;
    border-radius: var(--radius-sm);
    font-size: 13px;
    font-weight: 500;
    color: var(--text-2);
    background: var(--s3);
  }
  .origin {
    font-size: 13px;
    color: var(--text-2);
  }
  .deny .action {
    color: var(--danger);
  }
  .auto .action {
    color: var(--warn);
    background: var(--warn-soft);
  }
  .line button {
    padding: 2px 8px;
    color: var(--text-2);
    background: transparent;
  }
  .line button:first-of-type {
    margin-left: auto;
  }
  .notes {
    margin: 4px 0 0;
    padding-left: 16px;
    color: var(--warn);
    font-size: 13px;
  }
  form {
    display: flex;
    flex-wrap: wrap;
    align-items: end;
    gap: 8px;
    margin: 12px 0 4px;
  }
  fieldset {
    display: contents;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  label.check {
    flex-direction: row;
    align-items: center;
    align-self: center;
  }
  .quiet {
    margin: 0 0 12px;
  }
  .quiet label.check {
    color: var(--text);
    align-self: start;
  }
  .quiet p {
    margin: 4px 0 0;
  }
  @media (forced-colors: active) {
    button,
    input,
    select {
      border: 1px solid ButtonText;
    }
  }
</style>
