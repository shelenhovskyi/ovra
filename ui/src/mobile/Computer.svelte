<script lang="ts">
// The computer this phone is paired with: keep awake, how many agents work
// against the limit, notifications, and Forget this phone.
import { onMount } from "svelte";
import type { KeepAwakeMode } from "../lib/protocol";
import Header from "./Header.svelte";
import { errorText, type Phone } from "./phone.svelte";
import { browserPushSupport, pushOn, turnPushOff, turnPushOn, unsubscribeHere } from "./push";

let { phone, name, onForgotten }: { phone: Phone; name: string; onForgotten: () => void } = $props();

const MODES: { mode: KeepAwakeMode; label: string }[] = [
  { mode: "off", label: "Off" },
  { mode: "while_agents_work", label: "While agents work" },
  { mode: "always", label: "Always, until Ovra quits" },
];

const support = browserPushSupport();
let notifying = $state(false);
let pushBusy = $state(false);
let pushError = $state<string | null>(null);
let confirming = $state(false);
let forgetError = $state<string | null>(null);

onMount(() => {
  pushOn().then(
    (on) => (notifying = on),
    () => {},
  );
});

async function togglePush(): Promise<void> {
  if (pushBusy) return;
  pushBusy = true;
  pushError = null;
  try {
    if (notifying) {
      await turnPushOff(phone);
      notifying = false;
    } else {
      pushError = await turnPushOn(phone);
      notifying = pushError === null;
    }
  } catch (err) {
    pushError = `Notifications could not be changed: ${errorText(err)}`;
  }
  pushBusy = false;
}

async function forget(): Promise<void> {
  try {
    await phone.phoneCall({ method: "phone.forget" });
    void unsubscribeHere().catch(() => {});
    onForgotten();
  } catch (err) {
    forgetError = `The computer did not forget this phone: ${errorText(err)}`;
  }
}
</script>

<Header title="Computer" status={phone.status} />

{#if phone.error}<p class="error" role="alert">{phone.error}</p>{/if}

<section aria-labelledby="awake">
  <h2 id="awake">Keep awake</h2>
  {#if phone.power}
    <div role="radiogroup" aria-labelledby="awake">
      {#each MODES as { mode, label } (mode)}
        <label class="choice">
          <input type="radio" name="keep-awake" checked={phone.power.mode === mode} onchange={() => void phone.setMode(mode)} />
          {label}
        </label>
      {/each}
    </div>
    <p class="note">
      {phone.power.problem ?? (phone.power.awake ? "The computer stays awake now." : "The computer may sleep now.")}
    </p>
  {/if}
</section>

<section aria-labelledby="agents">
  <h2 id="agents">Agents</h2>
  <ul data-testid="capacity">
    {#each phone.projects.filter((p) => phone.capacity.has(p.id)) as project (project.id)}
      {@const c = phone.capacity.get(project.id)}
      {#if c && (c.working > 0 || c.queued > 0)}
        <li>
          {project.name}: {c.working} of {c.limit} working{c.queued > 0 ? `, ${c.queued} waiting` : ""}
        </li>
      {/if}
    {/each}
  </ul>
  {#if ![...phone.capacity.values()].some((c) => c.working > 0 || c.queued > 0)}
    <p class="note">No agent works right now.</p>
  {/if}
</section>

<section aria-labelledby="notifications">
  <h2 id="notifications">Notifications</h2>
  {#if support === "ready"}
    <label class="choice">
      <input type="checkbox" role="switch" checked={notifying} disabled={pushBusy} onchange={() => void togglePush()} />
      Tell me when an agent needs me
    </label>
    <p class="note">Notifications only say that something waits, never what. They pass through your phone maker's push service.</p>
  {:else if support === "add_to_home_screen"}
    <p class="note">To get notifications, tap Share, then Add to Home Screen, and open Ovra from there. The Home Screen app pairs on its own, with a new code from the computer.</p>
  {:else}
    <p class="note">This browser cannot show notifications.</p>
  {/if}
  {#if pushError}<p class="error" role="alert">{pushError}</p>{/if}
</section>

<section aria-labelledby="this-phone">
  <h2 id="this-phone">This phone</h2>
  <p class="note">Paired as {name}.</p>
  {#if confirming}
    <p>After this, the phone cannot reach Ovra until you pair it again.</p>
    <div class="actions">
      <button type="button" onclick={() => (confirming = false)}>Cancel</button>
      <button type="button" class="danger" onclick={() => void forget()}>Forget</button>
    </div>
  {:else}
    <button type="button" class="danger wide" onclick={() => (confirming = true)}>Forget this phone</button>
  {/if}
  {#if forgetError}<p class="error" role="alert">{forgetError}</p>{/if}
</section>

<style>
  h2 {
    font: var(--t-title2);
    margin: var(--sp-6) 0 var(--sp-2);
  }
  ul {
    margin: 0;
    padding-left: var(--sp-5);
  }
  .choice {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    min-height: 44px;
  }
  .choice input {
    width: 22px;
    height: 22px;
  }
  .note {
    color: var(--text-2);
  }
  .error {
    color: var(--danger);
  }
  .wide {
    width: 100%;
  }
  .actions {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--sp-3);
  }
</style>
