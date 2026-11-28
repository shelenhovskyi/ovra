<script lang="ts">
// Settings > Phone: turn phone access on, pair a phone with a QR code and
// forget phones. Off until the person turns it on; only this Wi-Fi.
import { untrack } from "svelte";
import { useOvra } from "../shell/context";
import { PhoneAccess, seen, timeLeft } from "./phone.svelte";

const { transport } = useOvra();
const phone = new PhoneAccess(transport);

$effect(() => untrack(() => phone.follow()));

let address = $state("");
const addresses = $derived(phone.status?.addresses ?? []);
const on = $derived(!!phone.status?.enabled);

/** Seconds left on the pairing code, counted down while it shows. */
let left = $state(0);
$effect(() => {
  const pairing = phone.pairing;
  if (!pairing) return;
  const until = Date.now() + pairing.expires_in * 1000;
  left = pairing.expires_in;
  const timer = setInterval(() => {
    left = Math.round((until - Date.now()) / 1000);
    if (left <= 0) phone.pairing = null;
  }, 1000);
  return () => clearInterval(timer);
});

const qr = $derived(
  phone.pairing ? `data:image/svg+xml;charset=utf-8,${encodeURIComponent(phone.pairing.qr_svg)}` : "",
);
</script>

{#if phone.status}
  <div class="phone" data-testid="phone-pane">
    <label class="check">
      <input
        type="checkbox"
        checked={on}
        onchange={(e) => void phone.setEnabled(e.currentTarget.checked, address || undefined)}
        data-testid="phone-enabled"
      />
      Let my phones connect over this Wi-Fi
    </label>
    {#if addresses.length > 1}
      <label class="field">
        <span>Network address</span>
        <select bind:value={address} onchange={() => on && void phone.setEnabled(true, address || undefined)}>
          {#each addresses as a (a)}<option value={a}>{a}</option>{/each}
        </select>
      </label>
    {/if}
    {#if on && phone.status.url}
      <p class="hint" data-testid="phone-url">Phones open {phone.status.url}</p>
    {:else if on && phone.status.problem}
      <p class="error" role="alert">{phone.status.problem}</p>
    {:else if !on && addresses.length === 0}
      <p class="hint">Connect this computer to your home or office Wi-Fi first.</p>
    {/if}

    {#if on && phone.status.url}
      {#if phone.pairing}
        <section class="pairing" aria-label="Pair a phone" data-testid="phone-pairing">
          <img src={qr} alt="QR code for pairing a phone" width="220" height="220" />
          <ol>
            <li>
              The first time, open <strong class="mono">{phone.pairing.certificate_url}</strong> on the phone and install
              Ovra's certificate. Its fingerprint ends in
              <strong class="mono">{phone.pairing.certificate_fingerprint.split(" ").slice(-2).join(" ")}</strong>.
            </li>
            <li>Scan the code with the phone's camera.</li>
            <li>Allow the phone here when it shows the same four digits.</li>
          </ol>
          <p class="hint" data-testid="phone-pairing-left">The code works for {timeLeft(left)}, for one phone.</p>
          <button type="button" onclick={() => (phone.pairing = null)}>Done</button>
        </section>
      {:else}
        <button type="button" class="primary" onclick={() => void phone.startPairing()} data-testid="phone-pair"
          >Pair a phone</button
        >
      {/if}
    {/if}

    {#if phone.devices.length > 0}
      <h4>Paired phones</h4>
      <ul class="devices" data-testid="phone-devices">
        {#each phone.devices as device (device.id)}
          <li>
            <span class="name">{device.name}</span>
            <span class="seen">{seen(device)}</span>
            <button type="button" onclick={() => void phone.forget(device.id)} aria-label={`Forget ${device.name}`}
              >Forget</button
            >
          </li>
        {/each}
      </ul>
    {/if}
    {#if phone.error}<p class="error" role="alert">{phone.error}</p>{/if}
    <p class="hint">
      A paired phone can see your agents and tasks, answer their requests, send them messages and stop them. It can't
      open files, run commands, start agents or change settings. Phone access turns itself off after 7 days without a
      phone, and a phone not seen for 30 days is forgotten.
    </p>
  </div>
{/if}

<style>
  .phone {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    max-width: 480px;
  }
  .check {
    display: flex;
    align-items: center;
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
  p,
  ol {
    margin: 0;
  }
  ol {
    padding-left: var(--sp-4);
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
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
    font-weight: 500;
    word-break: break-all;
  }
  .pairing {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    align-items: flex-start;
    margin-top: var(--sp-2);
  }
  .pairing img {
    padding: var(--sp-2);
    background: #fff;
    border-radius: var(--r-md);
  }
  h4 {
    margin: var(--sp-3) 0 0;
    font: var(--t-small);
    font-weight: 600;
    color: var(--text-2);
  }
  .devices {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .devices li {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    height: var(--row);
  }
  .name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .seen {
    font: var(--t-small);
    color: var(--text-2);
  }
  button {
    height: var(--ctl);
    padding: 0 var(--sp-3);
    font: inherit;
    color: var(--text);
    background: var(--s3);
    border: 0;
    border-radius: var(--r-control);
    cursor: default;
  }
  button.primary {
    align-self: flex-start;
    font-weight: 500;
    color: var(--accent-ink);
    background: var(--accent);
  }
</style>
