<script lang="ts">
// Settings > Editor: text size, tabs, word wrap and auto save. Every field
// reads and writes `editorPrefs`, which checks the value on write; a number
// field shows the kept value again after a write that was clamped.
import { type AutoSave, editorPrefs } from "../editor/prefs.svelte";

const AUTO_SAVE: { id: AutoSave; label: string }[] = [
  { id: "off", label: "Off" },
  { id: "after_delay", label: "After a delay" },
  { id: "on_focus_change", label: "When focus leaves" },
];

/** Writes a number field and shows what `editorPrefs` kept. */
function setNumber(input: HTMLInputElement, field: "fontSize" | "tabSize"): void {
  editorPrefs[field] = Number.parseFloat(input.value);
  input.value = String(editorPrefs[field]);
}
</script>

<div class="fields">
  <label class="field">
    <span>Text size</span>
    <input
      type="number"
      min="10"
      max="24"
      step="0.5"
      value={editorPrefs.fontSize}
      onchange={(e) => setNumber(e.currentTarget, "fontSize")}
      data-testid="editor-font-size"
    />
  </label>
  <label class="field">
    <span>Tab size</span>
    <input
      type="number"
      min="1"
      max="16"
      step="1"
      value={editorPrefs.tabSize}
      onchange={(e) => setNumber(e.currentTarget, "tabSize")}
      data-testid="editor-tab-size"
    />
  </label>
  <label class="check">
    <input type="checkbox" bind:checked={editorPrefs.insertSpaces} data-testid="editor-spaces" />
    Indent with spaces
  </label>
  <label class="check">
    <input type="checkbox" role="switch" bind:checked={editorPrefs.wordWrap} data-testid="editor-wrap" />
    Word wrap
  </label>
  <label class="field">
    <span>Auto save</span>
    <select bind:value={editorPrefs.autoSave} data-testid="editor-auto-save">
      {#each AUTO_SAVE as mode (mode.id)}
        <option value={mode.id}>{mode.label}</option>
      {/each}
    </select>
  </label>
  <p class="note">Auto save never overwrites a file changed on disk.</p>
</div>

<style>
  .fields {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    max-width: 480px;
  }
  .field {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
  }
  .field span {
    min-width: 160px;
    color: var(--text-2);
  }
  .check {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
  }
  input[type="number"],
  select {
    box-sizing: border-box;
    height: var(--ctl);
    padding: 0 var(--sp-2);
    font: inherit;
    color: var(--text);
    background: var(--s3);
    border: 0;
    border-radius: var(--r-md);
  }
  input[type="number"] {
    width: 80px;
    font-variant-numeric: tabular-nums;
  }
  .note {
    margin: 0;
    font: var(--t-small);
    color: var(--text-2);
  }
</style>
