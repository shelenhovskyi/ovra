<script lang="ts">
import { guessName, type Identity } from "./identity";
import { type PairStep, pair } from "./pair";

let {
  origin,
  secret,
  post,
  onPaired,
  homeScreenFirst = false,
}: {
  origin: string;
  secret: string;
  post: typeof fetch;
  onPaired: (identity: Identity) => void;
  /** Safari on an iPhone: notifications need the Home Screen app, which pairs on its own. */
  homeScreenFirst?: boolean;
} = $props();

let name = $state(guessName(navigator.userAgent));
let step = $state<PairStep | null>(null);
let busy = $state(false);
let copied = $state<"no" | "yes" | "failed">("no");

async function copyLink(): Promise<void> {
  try {
    await navigator.clipboard.writeText(`${origin}/#pair=${secret}`);
    copied = "yes";
  } catch {
    copied = "failed";
  }
}

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
    {#if homeScreenFirst}
      <div class="first" data-testid="home-screen-first">
        <p>
          For notifications, use Ovra from the Home Screen. It pairs on its own: copy this link, tap Share, then Add
          to Home Screen, open Ovra there and paste the link. The link works for 5 minutes.
        </p>
        <button type="button" onclick={() => void copyLink()}>Copy link</button>
        {#if copied === "yes"}<p role="status">Copied.</p>{/if}
        {#if copied === "failed"}<p role="alert">The link could not be copied. In the camera, press and hold the code's link and choose Copy instead.</p>{/if}
        <p>Or pair here in the browser, without notifications:</p>
      </div>
    {/if}
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
  .first {
    display: grid;
    gap: var(--sp-2);
    margin-bottom: var(--sp-4);
  }
  .code {
    font: 600 44px / 52px var(--mono);
    letter-spacing: 0.2em;
    color: var(--text);
    text-align: center;
    margin: var(--sp-6) 0;
  }
</style>
