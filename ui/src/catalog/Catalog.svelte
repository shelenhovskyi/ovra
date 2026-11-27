<script lang="ts">
// "Skills and instructions", a main-area view: everything the open
// project's files give its agents (instructions files, skills, subagents,
// commands, hooks, tool servers), listed by kind on the left with what each
// kind is for, and the chosen one on the right: what it is for, who reads
// it, its settings, its files and its text. It only reads; "Open file"
// opens the file in the editor. Read again when one of its files changes.
// "Teach the project" (Teach.svelte) writes AGENTS.md from the person's
// answers, in place of the detail.
import { untrack } from "svelte";
import type { CatalogItem } from "../lib/protocol";
import { CoreError } from "../lib/transport";
import { openSettings } from "../settings/commands";
import { useOvra } from "../shell/context";
import EmptyState from "../shell/EmptyState.svelte";
import { mainView } from "../shell/main-view.svelte";
import ViewFrame from "../shell/ViewFrame.svelte";
import MarkdownView from "../viewers/MarkdownView.svelte";
import Teach from "./Teach.svelte";
import {
  AGENT_NAMES,
  fieldLabel,
  groups,
  KIND_NAMES,
  loadsLine,
  showsText,
  title,
  touchesCatalog,
  withoutHeader,
} from "./texts";

const { transport, workspace } = useOvra();

let items = $state<CatalogItem[] | null>(null);
let error = $state<string | null>(null);
let filter = $state("");
let chosen = $state<string | null>(null);
/** The chosen item's text, for the path it was read from. */
let text = $state<{ path: string; text: string | null; error?: string } | null>(null);
let notice = $state("");
let teaching = $state(false);
let latest = 0;

const project = $derived(workspace.project);
const listed = $derived(groups(items ?? [], filter));
const flat = $derived(listed.flatMap((group) => group.items));
const picked = $derived(flat.find((entry) => entry.key === chosen) ?? flat[0] ?? null);
const item = $derived(picked?.item ?? null);
const loads = $derived(item ? loadsLine(item.loads, project?.trusted === true) : null);

$effect(() => {
  const id = project?.id ?? null;
  untrack(() => {
    items = null;
    error = null;
    chosen = null;
    teaching = false;
    if (id) void load(id);
  });
});

$effect(() =>
  transport.onEvent((event) => {
    const id = project?.id;
    if (id && event.type === "fs.changed" && event.data.project === id && touchesCatalog(event.data.paths)) {
      void load(id);
      if (item && event.data.paths.includes(item.path)) text = null;
    }
  }),
);

// The chosen Markdown file's text, read when it is chosen.
$effect(() => {
  const id = project?.id;
  const path = item && showsText(item) ? item.path : null;
  untrack(() => {
    notice = "";
    if (!id || !path || text?.path === path) return;
    text = { path, text: null };
    transport
      .call({ method: "fs.read_file", params: { project: id, path } })
      .then((reply) => {
        if (text?.path !== path) return;
        const body = reply.data.body;
        text =
          body.kind === "text"
            ? { path, text: withoutHeader(body.text) }
            : { path, text: null, error: "Not a text file." };
      })
      .catch((err) => {
        if (text?.path === path) text = { path, text: null, error: message(err) };
      });
  });
});

async function load(id: string): Promise<void> {
  const request = ++latest;
  try {
    const reply = await transport.call({ method: "catalog.list", params: { project: id } });
    if (request === latest && workspace.project?.id === id) {
      items = reply.data.items;
      error = null;
    }
  } catch (err) {
    if (request === latest) error = message(err);
  }
}

function message(err: unknown): string {
  return err instanceof CoreError ? err.error.message : String(err);
}

function open(path: string): void {
  workspace.openFile(path);
  // The same file open already: nothing else closes the view.
  mainView.close(false);
}

/** Teach the project saved `path`: its entry is shown once the list is read again. */
function taught(path: string): void {
  teaching = false;
  chosen = `rules|${path}|${path}|0`;
  text = null;
  const id = project?.id;
  if (id) void load(id);
}

function onListKeydown(event: KeyboardEvent): void {
  const at = flat.findIndex((entry) => entry.key === picked?.key);
  const moves: Record<string, number> = { ArrowDown: 1, ArrowUp: -1 };
  const next =
    event.code === "Home"
      ? flat[0]
      : event.code === "End"
        ? flat.at(-1)
        : event.code in moves
          ? flat[Math.max(0, Math.min(flat.length - 1, at + (moves[event.code] ?? 0)))]
          : undefined;
  if (!next) return;
  event.preventDefault();
  chosen = next.key;
  const list = event.currentTarget as HTMLElement;
  list.querySelector<HTMLElement>(`[data-key="${CSS.escape(next.key)}"]`)?.focus();
}
</script>

<ViewFrame name="catalog" testid="catalog">
  {#snippet tools()}
    {#if items && items.length > 0 && !teaching}
      <button type="button" class="teach-button" onclick={() => (teaching = true)} data-testid="catalog-teach-open"
        >Teach the project</button
      >
    {/if}
    {#if items && items.length > 0}
      <input
        type="search"
        placeholder="Filter"
        aria-label="Filter skills and instructions"
        bind:value={filter}
        data-testid="catalog-filter"
      />
    {/if}
  {/snippet}
  {#if !project}
    <EmptyState centred headline="No project open" line="Open a project to see what it gives its agents." />
  {:else if error}
    <p class="error note" role="alert">Could not read the project's agent files: {error}</p>
  {:else if !items}
    <p class="muted note">Reading the project's agent files…</p>
  {:else if teaching && items.length === 0}
    <Teach {transport} project={project.id} done={taught} cancel={() => (teaching = false)} />
  {:else if items.length === 0}
    <EmptyState
      centred
      testid="catalog-empty"
      headline="Nothing for agents here yet"
      line="Agents read instructions, skills and commands from files such as AGENTS.md, CLAUDE.md and .claude/skills. Add one and it shows here."
      actions={[{ label: "Teach the project", run: () => (teaching = true) }]}
    />
  {:else}
    <div class="catalog">
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <nav aria-label="Skills and instructions" onkeydown={onListKeydown}>
        {#if listed.length === 0}
          <p class="muted note">Nothing matches “{filter}”.</p>
        {/if}
        {#each listed as group (group.kind)}
          <section aria-labelledby={`catalog-${group.kind}`}>
            <h3 id={`catalog-${group.kind}`} class="group">{group.title} <span class="count">{group.items.length}</span></h3>
            <p class="kind-line">{group.line}</p>
            <ul>
              {#each group.items as entry (entry.key)}
                <li>
                  <button
                    type="button"
                    data-key={entry.key}
                    aria-current={entry.key === picked?.key ? "true" : undefined}
                    tabindex={entry.key === picked?.key ? 0 : -1}
                    onclick={() => (chosen = entry.key)}
                  >
                    <span class="name">{title(entry.item)}</span>
                    {#if entry.item.problem}<span class="dot" title={entry.item.problem}></span>{/if}
                  </button>
                </li>
              {/each}
            </ul>
          </section>
        {/each}
      </nav>
      {#if teaching}
        <Teach {transport} project={project.id} done={taught} cancel={() => (teaching = false)} />
      {:else if item}
        <article class="detail" aria-labelledby="catalog-item-title" data-testid="catalog-detail">
          <p class="kind">{KIND_NAMES[item.kind]} · {item.agents.map((a) => AGENT_NAMES[a]).join(", ")}</p>
          <h3 id="catalog-item-title" class="title">{title(item)}</h3>
          {#if item.kind === "hook" && title(item) !== item.name}<p class="muted"><code>{item.name}</code></p>{/if}
          {#if item.description}<p class="description">{item.description}</p>{/if}
          {#if item.problem}<p class="warn" data-testid="catalog-problem">{item.problem}</p>{/if}
          {#if loads}
            <p class="muted" data-testid="catalog-loads">
              {loads.text}
              {#if loads.trust}<button type="button" class="link" onclick={() => openSettings("trust")}>Trust…</button>{/if}
            </p>
          {/if}
          {#if item.fields.length > 0}
            <dl>
              {#each item.fields as field, i (i)}
                <dt>{fieldLabel(field.name)}</dt>
                <dd><code>{field.value}</code></dd>
              {/each}
            </dl>
          {/if}
          {#if item.files.length > 0}
            <h4>Files in this skill</h4>
            <ul class="files">
              {#each item.files as file (file)}
                <li><button type="button" class="link" onclick={() => open(file)}><code>{file}</code></button></li>
              {/each}
            </ul>
          {/if}
          <p class="where">
            <code>{item.path}</code>
            <button type="button" onclick={() => open(item.path)} data-testid="catalog-open">Open file</button>
          </p>
          {#if showsText(item) && text?.path === item.path}
            {#if text.error}
              <p class="error note">Could not read the file: {text.error}</p>
            {:else if text.text !== null}
              <div class="text" data-testid="catalog-text">
                <MarkdownView text={text.text} path={item.path} open={(path) => open(path)} notify={(m) => (notice = m)} />
              </div>
              {#if notice}<p class="muted note" role="status">{notice}</p>{/if}
            {/if}
          {/if}
        </article>
      {/if}
    </div>
  {/if}
</ViewFrame>

<style>
  .catalog {
    display: grid;
    grid-template-columns: minmax(200px, 260px) minmax(0, 1fr);
    flex: 1;
    min-height: 0;
    height: 100%;
  }
  nav {
    padding: var(--sp-2) var(--sp-2) var(--sp-4) var(--sp-3);
    overflow-y: auto;
    border-right: 1px solid var(--separator);
  }
  .group {
    display: flex;
    gap: var(--sp-1);
    align-items: baseline;
    margin: var(--sp-3) 0 0 var(--sp-2);
    font: var(--t-caption);
    font-weight: 600;
    color: var(--text-2);
  }
  .count {
    font-weight: 400;
  }
  .kind-line {
    margin: 0 0 var(--sp-1) var(--sp-2);
    font: var(--t-small);
    color: var(--text-2);
  }
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  nav button {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    width: 100%;
    height: var(--row);
    padding: 0 var(--sp-2);
    font: var(--t-body);
    text-align: left;
    color: var(--text);
    background: transparent;
    border: 0;
    border-radius: var(--r-sm);
    cursor: default;
  }
  nav button:hover {
    background: var(--s3);
  }
  nav button[aria-current="true"] {
    font-weight: 500;
    background: var(--sel-focus);
  }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .dot {
    flex: none;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--warn);
  }
  .detail {
    max-width: 720px;
    padding: var(--sp-3) var(--sp-5) var(--sp-6);
    overflow-y: auto;
  }
  .detail > p {
    margin: 0 0 var(--sp-2);
  }
  .kind {
    font: var(--t-caption);
    color: var(--text-2);
  }
  .title {
    margin: 0 0 var(--sp-2);
    font: var(--t-title2);
    letter-spacing: -0.02em;
    overflow-wrap: anywhere;
  }
  .muted {
    color: var(--text-2);
  }
  .warn {
    color: var(--warn);
  }
  .error {
    color: var(--danger);
  }
  .note {
    margin: var(--sp-3);
    font: var(--t-small);
  }
  dl {
    display: grid;
    grid-template-columns: max-content minmax(0, 1fr);
    gap: var(--sp-1) var(--sp-3);
    margin: var(--sp-3) 0;
  }
  dt {
    color: var(--text-2);
  }
  dd {
    margin: 0;
    overflow-wrap: anywhere;
  }
  h4 {
    margin: var(--sp-3) 0 var(--sp-1);
    font: var(--t-caption);
    font-weight: 600;
    color: var(--text-2);
  }
  .files li {
    margin: 0 0 2px;
  }
  .where {
    display: flex;
    flex-wrap: wrap;
    gap: var(--sp-2);
    align-items: center;
    margin-top: var(--sp-3) !important;
    color: var(--text-2);
  }
  .link {
    padding: 0;
    font: inherit;
    color: var(--accent);
    text-align: left;
    background: none;
    border: 0;
    cursor: default;
  }
  .link:hover {
    text-decoration: underline;
  }
  .text {
    margin-top: var(--sp-3);
    padding-top: var(--sp-3);
    border-top: 1px solid var(--separator);
  }
  input[type="search"] {
    width: 220px;
    height: var(--ctl);
    padding: 0 var(--sp-2);
    color: var(--text);
    background: var(--s3);
    border: 0;
    border-radius: var(--r-md);
  }
  .teach-button,
  .where button {
    height: var(--ctl);
    padding: 0 var(--sp-3);
    color: var(--text);
    background: var(--s3);
    border: 0;
    border-radius: var(--r-md);
    cursor: default;
  }
  .teach-button:hover,
  .where button:hover {
    background: var(--s4);
  }
  /* The viewer's page margins are the detail's here. */
  .text :global(.markdown) {
    padding: 0;
  }
  @media (forced-colors: active) {
    input[type="search"],
    .teach-button,
    .where button {
      border: 1px solid ButtonText;
    }
  }
</style>
