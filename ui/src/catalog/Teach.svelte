<script lang="ts">
// "Teach the project": three short questions whose answers go into the
// project's AGENTS.md, which every agent reads first. The commands are found
// in the project's own build files; the person checks and edits them. The
// file is written only on "Save to AGENTS.md", and the preview shows the
// whole file as it will be. When a CLAUDE.md exists (Claude then skips
// AGENTS.md), a checked box also puts `@AGENTS.md` at its top.
import { untrack } from "svelte";
import type { ProjectId } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";
import {
  type Answers,
  BUILD_FILES,
  detectCommands,
  importsRules,
  RULES_FILE,
  readText,
  withAnswers,
  withImport,
  writeText,
} from "./teach";

let {
  transport,
  project,
  done,
  cancel,
}: { transport: Transport; project: ProjectId; done: (path: string) => void; cancel: () => void } = $props();

type File = { text: string; mtime: number } | null;

let rules = $state<File | undefined>(undefined);
let claude = $state<File | undefined>(undefined);
let answers = $state<Answers>({ commands: "", rules: "", never: "" });
let linkClaude = $state(true);
let busy = $state(false);
let error = $state("");

const ready = $derived(rules !== undefined && claude !== undefined);
const preview = $derived(ready ? withAnswers(rules?.text ?? null, answers) : "");
const changed = $derived(ready && preview !== (rules?.text ?? null));
const asksLink = $derived(claude !== undefined && claude !== null && !importsRules(claude.text));
const blocked = $derived(busy || !ready || (!changed && !(asksLink && linkClaude)));

$effect(() => {
  const id = project;
  untrack(() => void load(id));
});

async function load(id: ProjectId): Promise<void> {
  try {
    const found: Partial<Record<string, string>> = {};
    const [own, claudeMd, ...builds] = await Promise.all([
      readText(transport, id, RULES_FILE),
      readText(transport, id, "CLAUDE.md"),
      ...BUILD_FILES.map((path) => readText(transport, id, path).catch(() => null)),
    ]);
    BUILD_FILES.forEach((path, i) => {
      const file = builds[i];
      if (file) found[path] = file.text;
    });
    if (id !== project) return;
    rules = own;
    claude = claudeMd;
    answers.commands = detectCommands(found).join("\n");
  } catch (err) {
    error = `Could not read the project: ${message(err)}`;
  }
}

function message(err: unknown): string {
  return err instanceof CoreError ? err.error.message : String(err);
}

async function save(): Promise<void> {
  if (blocked) return;
  busy = true;
  error = "";
  try {
    if (changed) await writeText(transport, project, RULES_FILE, preview, rules?.mtime ?? null);
    if (asksLink && linkClaude && claude) {
      await writeText(transport, project, "CLAUDE.md", withImport(claude.text), claude.mtime);
    }
    done(RULES_FILE);
  } catch (err) {
    error =
      err instanceof CoreError && err.error.code === "conflict"
        ? "The file changed while you were here. Close this and try again."
        : `Could not save: ${message(err)}`;
  } finally {
    busy = false;
  }
}
</script>

<form
  class="teach"
  data-testid="catalog-teach"
  aria-labelledby="teach-title"
  onsubmit={(e) => {
    e.preventDefault();
    void save();
  }}
>
  <h3 id="teach-title" class="title">Teach the project</h3>
  <p class="muted">
    Agents read <code>{RULES_FILE}</code> before every task. Tell them what they should know; each line becomes one
    item. Leave out what you are not sure about.
  </p>
  {#if !ready && !error}
    <p class="muted" role="status">Reading the project…</p>
  {:else if ready}
    <label for="teach-commands">How to build, run and test it</label>
    <p class="hint">Found in the project's build files. Remove what agents should not run.</p>
    <textarea id="teach-commands" rows="5" bind:value={answers.commands} data-testid="teach-commands"></textarea>
    <label for="teach-rules">Rules agents must follow</label>
    <p class="hint">For example: Every change comes with a test.</p>
    <textarea id="teach-rules" rows="3" bind:value={answers.rules} data-testid="teach-rules"></textarea>
    <label for="teach-never">What agents must not touch</label>
    <p class="hint">For example: Never edit the files in migrations/.</p>
    <textarea id="teach-never" rows="2" bind:value={answers.never} data-testid="teach-never"></textarea>
    {#if asksLink}
      <label class="check">
        <input type="checkbox" bind:checked={linkClaude} data-testid="teach-link" />
        Claude reads <code>CLAUDE.md</code> instead: also add <code>@{RULES_FILE}</code> at its top
      </label>
    {/if}
    <h4>{rules ? `${RULES_FILE} after saving` : `New ${RULES_FILE}`}</h4>
    <pre class="preview" data-testid="teach-preview">{preview}</pre>
  {/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  <div class="buttons">
    <button type="button" onclick={cancel}>Cancel</button>
    <button
      type="submit"
      class="primary"
      aria-disabled={blocked}
      data-testid="teach-save">Save to {RULES_FILE}</button
    >
  </div>
</form>

<style>
  .teach {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    max-width: 720px;
    padding: var(--sp-3) var(--sp-5) var(--sp-6);
    overflow-y: auto;
  }
  .title {
    margin: 0 0 var(--sp-1);
    font: var(--t-title2);
    letter-spacing: -0.02em;
  }
  p {
    margin: 0;
  }
  .muted,
  .hint {
    color: var(--text-2);
  }
  .hint {
    font: var(--t-small);
  }
  label {
    margin-top: var(--sp-3);
    font-weight: 500;
  }
  .check {
    display: flex;
    gap: var(--sp-2);
    align-items: center;
    font-weight: 400;
  }
  textarea {
    width: 100%;
    padding: var(--sp-2);
    font: inherit;
    box-sizing: border-box;
    color: var(--text);
    background: var(--s3);
    border: 0;
    border-radius: var(--r-md);
    resize: vertical;
  }
  h4 {
    margin: var(--sp-4) 0 var(--sp-1);
    font: var(--t-caption);
    font-weight: 600;
    color: var(--text-2);
  }
  .preview {
    max-height: 260px;
    margin: 0;
    padding: var(--sp-2) var(--sp-3);
    overflow: auto;
    font: var(--t-small);
    font-family: var(--mono);
    white-space: pre-wrap;
    background: var(--s2);
    border-radius: var(--r-md);
  }
  .error {
    margin-top: var(--sp-2);
    color: var(--danger);
  }
  .buttons {
    display: flex;
    gap: var(--sp-2);
    justify-content: flex-end;
    margin-top: var(--sp-3);
  }
  .buttons button {
    height: var(--ctl);
    padding: 0 var(--sp-3);
    color: var(--text);
    background: var(--s3);
    border: 0;
    border-radius: var(--r-control);
    cursor: default;
  }
  .buttons .primary {
    color: var(--accent-ink);
    font-weight: 500;
    background: var(--accent);
  }
  .buttons [aria-disabled="true"] {
    opacity: 0.5;
  }
  @media (forced-colors: active) {
    textarea,
    .buttons button {
      border: 1px solid ButtonText;
    }
  }
</style>
