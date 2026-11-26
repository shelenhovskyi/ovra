<script lang="ts">
import { guessName, type Identity } from "./identity";
import { type PairStep, pair } from "./pair";

let {
  origin,
  secret,
  post,
  onPaired,
}: { origin: string; secret: string; post: typeof fetch; onPaired: (identity: Identity) => void } = $props();

let name = $state(guessName(navigator.userAgent));
let step = $state<PairStep | null>(null);
let busy = $state(false);

async function start(event: SubmitEvent): Promise<void> {
  event.preventDefault();
  if (busy) return;
  busy = true;
  await pair(
    origin,
    secret,
    name.trim() || "Phone",
    (next) => {
      step = next;
      if (next.kind === "paired") onPaired(next.identity);
    },
    post,
  );
  busy = false;
}
</script>

<section aria-labelledby="pair-title">
  <h1 id="pair-title">Pair with Ovra</h1>
  {#if step?.kind === "waiting"}
    <p>Your computer now asks whether to allow this phone. Check that it shows the same code, then click Allow there.</p>
    <p class="code" data-testid="pair-code" aria-label="Code {step.code.split('').join(' ')}">{step.code}</p>
    <p role="status">Waiting for your computer…</p>
  {:else if step?.kind === "failed"}
    <p role="alert">{step.message}</p>
  {:else}
    <p>This phone will see your agents, answer their requests, reply to them and stop them.</p>
    <form onsubmit={start}>
      <label for="phone-name">Name of this phone</label>
      <input id="phone-name" type="text" bind:value={name} maxlength="40" autocomplete="off" />
      <button type="submit" class="primary" aria-disabled={busy}>Pair</button>
    </form>
  {/if}
</section>

<style>
  h1 {
    font: var(--t-title1);
  }
  p {
    color: var(--text-2);
  }
  form {
    display: grid;
    gap: var(--sp-3);
  }
  label {
    font: var(--t-headline);
  }
  .code {
    font: 600 44px / 52px var(--mono);
    letter-spacing: 0.2em;
    color: var(--text);
    text-align: center;
    margin: var(--sp-6) 0;
  }
</style>
