<script lang="ts">
// Settings > Remote machines (ADR 0016): the machines the person added,
// their trusted keys, and adding one. Adding never connects: Ovra shows the
// keys the person's own `known_hosts` has, and trusts exactly those.
import { untrack } from "svelte";
import type { PreparedHost, RemoteHost } from "../lib/protocol";
import type { RemoteLink } from "../lib/transport";
import { useOvra } from "../shell/context";
import { Machines } from "./machines.svelte";

const { transport } = useOvra();
const machines = new Machines(transport);

// A remote window's core has no remote machines; it shows a pointer instead.
$effect(() => (transport.remote ? undefined : untrack(() => machines.follow())));

let adding = $state(false);
let name = $state("");
let destination = $state("");
let daemon = $state("");
/** The keys found for the destination, shown before anything is trusted. */
let prepared = $state<PreparedHost | null>(null);
let renaming = $state<string | null>(null);
/** Each machine's link, from the app: only the laptop's own window hears them. */
const links = $state<Record<string, RemoteLink>>({});
$effect(() => untrack(() => transport.onRemoteLinks?.((id, link) => (links[id] = link))));
/** The machine whose restart waits for the person's confirmation. */
let restarting = $state<string | null>(null);
let newName = $state("");

function reset(): void {
  adding = false;
  name = destination = daemon = "";
  prepared = null;
  machines.error = null;
}

async function check(event: SubmitEvent): Promise<void> {
  event.preventDefault();
  prepared = await machines.prepare(destination.trim());
}

async function trust(): Promise<void> {
  if (!prepared) return;
  const added = await machines.add(name.trim() || destination.trim().slice(0, 40), prepared, daemon.trim());
  if (added) reset();
}

async function restart(host: RemoteHost): Promise<void> {
  restarting = null;
  await machines.restart(host);
}

/** Whether the machine's Ovra answered as another build, which a restart may fix. */
const otherVersion = (id: string) => links[id]?.state === "failed" && links[id]?.reason === "version_mismatch";

async function saveName(host: RemoteHost, event: SubmitEvent): Promise<void> {
  event.preventDefault();
  await machines.rename(host.id, newName.trim());
  if (!machines.error) renaming = null;
}
</script>

<div class="machines" data-testid="machines-pane">
  {#if transport.remote}
    <p class="hint">Remote machines are added and opened from Ovra's own window on this computer.</p>
  {:else if machines.missing}
    <p class="hint" data-testid="machines-missing">Remote machines need the Ovra app on this computer.</p>
  {:else if machines.hosts}
    <p class="hint">
      Run agents on another Linux computer you reach over SSH. Ovra connects with your own SSH setup. Your SSH keys stay
      on this computer.
    </p>

    {#if machines.hosts.length > 0}
      <ul class="hosts" data-testid="machines-list">
        {#each machines.hosts as host (host.id)}
          <li>
            {#if renaming === host.id}
              <form class="row" onsubmit={(e) => void saveName(host, e)}>
                <input aria-label={`New name for ${host.name}`} bind:value={newName} maxlength="40" />
                <button type="submit" class="primary">Save</button>
                <button type="button" onclick={() => (renaming = null)}>Cancel</button>
              </form>
            {:else}
              <div class="row">
                <span class="name">{host.name}</span>
                <span class="mono dest">{host.destination}</span>
                {#if transport.openRemote}
                  <button type="button" class="primary" onclick={() => void machines.open(host)} data-testid="machine-open"
                    >Open</button
                  >
                {/if}
              </div>
            {/if}
            {#if otherVersion(host.id) && transport.restartRemote}
              <div class="notice" data-testid="machine-other-version">
                {#if restarting === host.id}
                  <p>
                    Restart Ovra on {host.name}? Agents running there stop. Their sessions can be resumed later.
                  </p>
                  <div class="row">
                    <button type="button" class="danger" onclick={() => void restart(host)} data-testid="machine-restart-confirm"
                      >Restart</button
                    >
                    <button type="button" onclick={() => (restarting = null)}>Cancel</button>
                  </div>
                {:else}
                  <p>Ovra on {host.name} is a different version from this one.</p>
                  <div class="row">
                    <button type="button" onclick={() => (restarting = host.id)} data-testid="machine-restart"
                      >Restart Ovra on {host.name}</button
                    >
                  </div>
                {/if}
              </div>
            {/if}
            <details>
              <summary>Details</summary>
              <dl>
                <dt>Ovra on the machine</dt>
                <dd class="mono">{host.daemon}</dd>
                <dt>Trusted host keys</dt>
                {#each host.fingerprints as fingerprint (fingerprint)}<dd class="mono">{fingerprint}</dd>{/each}
              </dl>
              <div class="row">
                <button
                  type="button"
                  onclick={() => {
                    renaming = host.id;
                    newName = host.name;
                  }}>Rename</button
                >
                <button type="button" onclick={() => void machines.remove(host.id)} aria-label={`Remove ${host.name}`}
                  >Remove</button
                >
              </div>
            </details>
          </li>
        {/each}
      </ul>
    {/if}

    {#if !adding}
      <button type="button" class="primary" onclick={() => (adding = true)} data-testid="machine-add"
        >Add a machine</button
      >
    {:else if !prepared}
      <form class="add" onsubmit={(e) => void check(e)} aria-label="Add a machine" data-testid="machine-form">
        <label class="field">
          <span>Name</span>
          <input bind:value={name} maxlength="40" placeholder="Build server" />
        </label>
        <label class="field">
          <span>SSH destination</span>
          <input bind:value={destination} required class="mono" placeholder="dev@build.example" spellcheck="false" />
        </label>
        <p class="hint">As you type it after <span class="mono">ssh</span> in a terminal: a host from your SSH config or user@host.</p>
        <label class="field">
          <span>Ovra on the machine</span>
          <input bind:value={daemon} class="mono" placeholder="ovra-daemon" spellcheck="false" />
        </label>
        <div class="row">
          <button type="submit" class="primary">Next</button>
          <button type="button" onclick={reset}>Cancel</button>
        </div>
      </form>
    {:else}
      <section class="keys" aria-label="Host keys" data-testid="machine-keys">
        <p>
          <span class="mono">{prepared.destination}</span> is
          <span class="mono">{prepared.user}@{prepared.host_name}:{prepared.port}</span>.
        </p>
        {#if prepared.keys.length === 0}
          <p class="error" role="alert">
            Ovra found no key for this machine in your SSH known hosts. Connect once in a terminal with
            <span class="mono">ssh {prepared.destination}</span>, check the fingerprint it shows, then try again.
          </p>
          <div class="row"><button type="button" onclick={() => (prepared = null)}>Back</button></div>
        {:else}
          <p>Ovra will trust only these host keys, from your SSH known hosts. If one changes, Ovra refuses to connect.</p>
          <ul class="fingerprints">
            {#each prepared.keys as key (key.fingerprint)}
              <li><span class="kind">{key.kind}</span> <span class="mono">{key.fingerprint}</span></li>
            {/each}
          </ul>
          <div class="row">
            <button type="button" class="primary" onclick={() => void trust()} data-testid="machine-trust"
              >Trust and add</button
            >
            <button type="button" onclick={() => (prepared = null)}>Back</button>
          </div>
        {/if}
      </section>
    {/if}
    {#if machines.error}<p class="error" role="alert">{machines.error}</p>{/if}
  {/if}
</div>

<style>
  .machines {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    max-width: 560px;
  }
  p {
    margin: 0;
  }
  .hint {
    font: var(--t-small);
    color: var(--text-2);
  }
  .error {
    color: var(--danger);
  }
  .mono {
    font-family: var(--mono);
    word-break: break-all;
  }
  .hosts,
  .fingerprints {
    margin: 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
  }
  .name {
    font-weight: 600;
  }
  .dest {
    flex: 1;
    font: var(--t-small);
    font-family: var(--mono);
    color: var(--text-2);
  }
  details {
    font: var(--t-small);
  }
  summary {
    color: var(--text-2);
  }
  dl {
    margin: var(--sp-1) 0 var(--sp-2);
  }
  dt {
    color: var(--text-2);
  }
  dd {
    margin: 0 0 var(--sp-1);
  }
  .add,
  .keys {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
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
  .field input {
    flex: 1;
  }
  .notice {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    margin: var(--sp-1) 0;
    padding: var(--sp-2) var(--sp-3);
    background: var(--s2);
    border-radius: var(--r-md);
  }
  button.danger {
    color: var(--danger);
    background: var(--danger-soft);
  }
  .kind {
    color: var(--text-2);
  }
  button {
    height: var(--ctl);
    padding: 0 var(--sp-3);
    font: inherit;
    color: var(--text);
    background: var(--s3);
    border: 0;
    border-radius: var(--r-md);
    cursor: default;
  }
  button.primary {
    font-weight: 500;
    color: var(--accent-ink);
    background: var(--accent);
  }
  .machines > button {
    align-self: flex-start;
  }
</style>
