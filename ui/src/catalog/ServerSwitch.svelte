<script lang="ts">
// Whether Ovra's agents get one server of the project's .mcp.json
// (docs/specs/project-mcp.md). Off until the person turns it on, after a
// confirmation that says what it runs or reaches; a change to the file
// turns it off until they turn it on again. Once a session had it, its
// tools are listed with a switch each.
import type { CatalogItem, ProjectId, ServerUse } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";

let {
  transport,
  project,
  item,
  use,
  changed,
}: {
  transport: Transport;
  project: ProjectId;
  item: CatalogItem;
  use: ServerUse;
  /** Something was turned on or off: the list is read again. */
  changed: () => void;
} = $props();

let asking = $state(false);
let busy = $state(false);
let error = $state("");

async function call(run: () => Promise<unknown>): Promise<void> {
  if (busy) return;
  busy = true;
  error = "";
  try {
    await run();
    asking = false;
    changed();
  } catch (err) {
    error = err instanceof CoreError ? err.error.message : String(err);
  } finally {
    busy = false;
  }
}

function setServer(on: boolean): Promise<void> {
  return call(() => transport.call({ method: "catalog.set_server", params: { project, name: item.name, on } }));
}

function setTool(tool: string, on: boolean): Promise<void> {
  return call(() => transport.call({ method: "catalog.set_tool", params: { project, server: item.name, tool, on } }));
}
</script>

<section class="use" aria-labelledby="server-use-title" data-testid="server-use">
  <h4 id="server-use-title">In Ovra</h4>
  {#if use.blocked}
    <p class="muted" data-testid="server-blocked">Agents in Ovra cannot get this server yet. {use.blocked}</p>
  {:else}
    {#if use.changed}
      <p class="warn" data-testid="server-changed">
        This server changed in <code>.mcp.json</code> since you turned it on, so agents no longer get it. Look at it
        and turn it on again if it is still right.
      </p>
    {/if}
    <label class="switch">
      <input
        type="checkbox"
        role="switch"
        checked={use.on}
        disabled={busy}
        data-testid="server-switch"
        onclick={(e) => {
          e.preventDefault();
          if (use.on) void setServer(false);
          else asking = true;
        }}
      />
      Give agents in this project this server
    </label>
    {#if asking}
      <div class="confirm" role="group" aria-label="Turn the server on" data-testid="server-confirm">
        <p>
          {#if use.host}
            Agents get its tools, which reach <code>{use.host}</code>. That host goes on this project's network list
            while the server is on.
          {:else}
            Agents get its tools. It runs <code>{item.description}</code> inside the agent's sandbox, with the same
            files and network the agent has.
          {/if}
          Every tool call still asks you first, and a change to <code>.mcp.json</code> turns it off again.
        </p>
        <div class="buttons">
          <button type="button" onclick={() => (asking = false)}>Cancel</button>
          <button type="button" class="primary" disabled={busy} onclick={() => void setServer(true)}
            data-testid="server-turn-on">Turn on</button
          >
        </div>
      </div>
    {/if}
    {#if use.on}
      {#if use.tools.length > 0}
        <ul class="tools" aria-label="Its tools">
          {#each use.tools as tool (tool.name)}
            <li>
              <label>
                <input
                  type="checkbox"
                  checked={tool.on}
                  disabled={busy}
                  onclick={(e) => {
                    e.preventDefault();
                    void setTool(tool.name, !tool.on);
                  }}
                />
                <code>{tool.name}</code>
              </label>
            </li>
          {/each}
        </ul>
      {:else}
        <p class="muted">Its tools show here, each with a switch, once an agent session started with it.</p>
      {/if}
    {/if}
  {/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
</section>

<style>
  .use {
    margin: var(--sp-3) 0;
  }
  h4 {
    margin: var(--sp-3) 0 var(--sp-1);
    font: var(--t-caption);
    font-weight: 600;
    color: var(--text-2);
  }
  p {
    margin: 0 0 var(--sp-2);
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
  label {
    display: flex;
    gap: var(--sp-2);
    align-items: center;
  }
  input[type="checkbox"] {
    accent-color: var(--accent);
  }
  .confirm {
    margin: var(--sp-2) 0;
    padding: var(--sp-2) var(--sp-3);
    background: var(--s2);
    border-radius: var(--r-md);
  }
  .buttons {
    display: flex;
    gap: var(--sp-2);
    justify-content: flex-end;
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
  .buttons .primary:not(:disabled) {
    color: var(--accent-ink);
    font-weight: 500;
    background: var(--accent);
  }
  .tools {
    margin: var(--sp-2) 0 0;
    padding: 0;
    list-style: none;
  }
  .tools li {
    margin: 0 0 2px;
  }
  @media (forced-colors: active) {
    .buttons button {
      border: 1px solid ButtonText;
    }
  }
</style>
