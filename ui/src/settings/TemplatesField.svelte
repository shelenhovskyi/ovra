<script lang="ts">
// Settings > Agents > Task templates (docs/specs/task-commands.md): the
// person's own task texts, which `/` in a message box offers. Kept in the
// page's storage with the other agent preferences.
import { agentPrefs, MAX_TEMPLATE_NAME, MAX_TEMPLATES } from "../agents/prefs.svelte";

/** The template being edited: an id, "new", or null while none is. */
let editing = $state<string | null>(null);
let name = $state("");
let text = $state("");
let nameField = $state<HTMLInputElement>();

const full = $derived(agentPrefs.templates.length >= MAX_TEMPLATES);
const canSave = $derived(name.trim() !== "" && text.trim() !== "");

function edit(id: string | "new"): void {
  const template = agentPrefs.templates.find((t) => t.id === id);
  editing = id;
  name = template?.name ?? "";
  text = template?.text ?? "";
  queueMicrotask(() => nameField?.focus());
}

function save(): void {
  if (!canSave || editing === null) return;
  if (editing === "new") agentPrefs.addTemplate(name, text);
  else agentPrefs.updateTemplate(editing, name, text);
  editing = null;
}

function firstLine(value: string): string {
  return value.trim().split("\n", 1)[0] ?? "";
}
</script>

{#snippet form()}
  <form
    class="edit"
    data-testid="template-form"
    onsubmit={(event) => {
      event.preventDefault();
      save();
    }}
  >
    <label>
      <span>Name</span>
      <input bind:this={nameField} bind:value={name} maxlength={MAX_TEMPLATE_NAME} placeholder="Review for bugs" />
    </label>
    <label>
      <span>Task</span>
      <textarea bind:value={text} rows="4" placeholder="Review the uncommitted changes for bugs and missing tests."
      ></textarea>
    </label>
    <div class="actions">
      <button type="submit" disabled={!canSave}>Save template</button>
      <button type="button" class="quiet" onclick={() => (editing = null)}>Cancel</button>
    </div>
  </form>
{/snippet}

<section class="templates" aria-labelledby="task-templates-title" data-testid="task-templates">
  <h4 id="task-templates-title">Task templates</h4>
  <p class="hint">Type <kbd>/</kbd> at the start of a task or reply to use one, or one of the project's commands.</p>
  {#if agentPrefs.templates.length > 0}
    <ul>
      {#each agentPrefs.templates as template (template.id)}
        <li data-testid="template-row">
          {#if editing === template.id}
            {@render form()}
          {:else}
            <span class="name">{template.name}</span>
            <span class="line">{firstLine(template.text)}</span>
            <button type="button" class="quiet" aria-label="Edit {template.name}" onclick={() => edit(template.id)}
              >Edit</button
            >
            <button
              type="button"
              class="quiet"
              aria-label="Delete {template.name}"
              onclick={() => agentPrefs.removeTemplate(template.id)}>Delete</button
            >
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
  {#if editing === "new"}
    {@render form()}
  {:else if !full}
    <button type="button" class="quiet add" onclick={() => edit("new")}>Add template…</button>
  {/if}
</section>

<style>
  .templates {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    max-width: 480px;
    margin-top: var(--sp-3);
  }
  h4 {
    margin: 0;
    font: inherit;
    font-weight: 500;
  }
  .hint {
    margin: 0;
    font: var(--t-small);
    color: var(--text-2);
  }
  ul {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  li {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    min-width: 0;
  }
  .name {
    flex: 0 0 auto;
    max-width: 40%;
    overflow: hidden;
    font-weight: 500;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .line {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    color: var(--text-2);
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .edit {
    display: flex;
    flex: 1 1 auto;
    flex-direction: column;
    gap: var(--sp-2);
    padding: var(--sp-2) 0;
  }
  .edit label {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .edit label span {
    font: var(--t-small);
    color: var(--text-2);
  }
  .edit textarea {
    resize: vertical;
  }
  .actions {
    display: flex;
    gap: var(--sp-2);
  }
  .add {
    align-self: flex-start;
  }
  /* A component of its own: the Agents pane's button styles do not reach it. */
  .quiet {
    height: var(--ctl);
    padding: 0 var(--sp-3);
    font: inherit;
    color: var(--text-2);
    background: transparent;
    border: 0;
    border-radius: var(--r-md);
  }
  .quiet:hover {
    color: var(--text);
    background: var(--s4);
  }
</style>
