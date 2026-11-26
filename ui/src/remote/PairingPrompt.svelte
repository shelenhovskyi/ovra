<script lang="ts">
// Asks the person, wherever they are in Ovra, whether the phone that used the
// pairing code may connect; and, while phone access is on, says so in the
// footer. Deny has the focus: allowing takes a deliberate choice.
import { tick, untrack } from "svelte";
import { openSettings } from "../settings/commands";
import { useOvra } from "../shell/context";
import { PhoneAccess } from "./phone.svelte";

const { transport } = useOvra();
const phone = new PhoneAccess(transport);

$effect(() => untrack(() => phone.follow()));

let deny = $state<HTMLButtonElement>();
$effect(() => {
  if (!phone.request) return;
  void tick().then(() => deny?.focus());
});

const connected = $derived(phone.status?.connected ?? 0);
</script>

{#if phone.status?.enabled}
  <button type="button" class="indicator" onclick={() => openSettings("phone")} data-testid="phone-indicator"
    >{connected === 0 ? "Phone access on" : connected === 1 ? "1 phone connected" : `${connected} phones connected`}</button
  >
{/if}
{#if phone.request}
  <div class="prompt" role="alertdialog" aria-labelledby="pair-title" aria-describedby="pair-line" data-testid="pairing-prompt">
    <h2 id="pair-title">Let “{phone.request.name}” connect?</h2>
    <p id="pair-line">Allow it only if your phone shows the same code:</p>
    <p class="code" data-testid="pairing-code">{phone.request.code}</p>
    <p class="hint">It will see your agents, answer their requests, send them messages and stop them.</p>
    <div class="actions">
      <button type="button" bind:this={deny} onclick={() => void phone.answer(false)}>Deny</button>
      <button type="button" class="primary" onclick={() => void phone.answer(true)}>Allow</button>
    </div>
  </div>
{/if}

<style>
  .indicator {
    height: 100%;
    padding: 0 var(--sp-2);
    font: var(--t-caption);
    color: var(--text-2);
    background: transparent;
    border: 0;
    cursor: default;
  }
  .indicator:hover {
    color: var(--text);
  }
  .prompt {
    position: fixed;
    top: 20%;
    left: 50%;
    z-index: 50;
    width: min(360px, calc(100vw - 32px));
    padding: var(--sp-4);
    transform: translateX(-50%);
    background: var(--s1);
    border-radius: var(--r-lg);
    box-shadow: var(--shadow);
  }
  h2 {
    margin: 0 0 var(--sp-2);
    font: var(--t-title3);
    font-weight: 600;
  }
  p {
    margin: 0 0 var(--sp-2);
  }
  .code {
    font-size: 32px;
    font-variant-numeric: tabular-nums;
    letter-spacing: 0.2em;
    text-align: center;
  }
  .hint {
    font: var(--t-small);
    color: var(--text-2);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--sp-2);
  }
  .actions button {
    height: var(--ctl);
    padding: 0 var(--sp-3);
    font: inherit;
    color: var(--text);
    background: var(--s3);
    border: 0;
    border-radius: var(--r-md);
    cursor: default;
  }
  .actions .primary {
    font-weight: 500;
    color: var(--accent-ink);
    background: var(--accent);
  }
  @media (forced-colors: active) {
    .prompt {
      border: 1px solid CanvasText;
    }
  }
</style>
