<script lang="ts">
// The phone app: pairs once, then connects with its key and shows the
// screen the URL's hash names.
import { onDestroy } from "svelte";
import Agent from "./Agent.svelte";
import Approval from "./Approval.svelte";
import Computer from "./Computer.svelte";
import Home from "./Home.svelte";
import type { Identity, IdentityStore } from "./identity";
import Pair from "./Pair.svelte";
import { pairLink, pairSecret } from "./pair";
import { Phone } from "./phone.svelte";
import { homeScreen as browserHomeScreen } from "./push";
import { parseRoute } from "./route";
import { connectPhone, type RawSocket, socketUrl } from "./socket";
import Task from "./Task.svelte";

let {
  store,
  origin,
  openSocket = (url: string) => new WebSocket(url),
  post = fetch,
  homeScreen = browserHomeScreen(),
}: {
  store: IdentityStore;
  origin: string;
  openSocket?: (url: string) => RawSocket;
  post?: typeof fetch;
  homeScreen?: { standalone: boolean; appleMobile: boolean };
} = $props();

let pasted = $state("");
let pasteError = $state<string | null>(null);

function pairFromLink(event: SubmitEvent): void {
  event.preventDefault();
  const next = pairLink(pasted, origin);
  pasteError = next ? null : "This is not a pairing link for this computer. Copy the link from a new code.";
  if (next) {
    secret = next;
    pasted = "";
  }
}

let hash = $state(location.hash);
const onHash = () => (hash = location.hash);
window.addEventListener("hashchange", onHash);
onDestroy(() => window.removeEventListener("hashchange", onHash));

/** `loading` until the store answers; `none`: not paired; `gone`: the computer forgot this phone. */
let identity = $state<Identity | null | "loading" | "gone">("loading");
let phone = $state<Phone | null>(null);
// Read once: the secret leaves the address bar before anything else runs,
// and is dropped once used.
const found = pairSecret(location.hash);
let secret = $state(found);
if (found) history.replaceState(null, "", `${location.pathname}#/`);

// The store does not change while the app runs.
// svelte-ignore state_referenced_locally
store.load().then(
  (kept) => {
    // A key signs for the origin it was paired with only.
    identity = kept && kept.origin === origin ? kept : null;
  },
  () => (identity = null),
);

$effect(() => {
  if (typeof identity !== "object" || identity === null || phone) return;
  const id = identity;
  phone = new Phone(
    connectPhone(
      id,
      () => openSocket(socketUrl(id.origin)),
      () => void forgotten(),
    ),
  );
});

async function forgotten(): Promise<void> {
  await store.clear().catch(() => {});
  phone = null;
  identity = "gone";
}

async function paired(next: Identity): Promise<void> {
  await store.save(next);
  secret = null;
  identity = next;
}

const route = $derived(parseRoute(hash));
</script>

<main>
  {#if identity === "loading"}
    <p class="center" role="status">Loading…</p>
  {:else if secret && (identity === null || identity === "gone")}
    <Pair
      {origin}
      {secret}
      {post}
      onPaired={paired}
      homeScreenFirst={homeScreen.appleMobile && !homeScreen.standalone}
    />
  {:else if identity === null || identity === "gone"}
    <section class="center" aria-labelledby="unpaired">
      <h1 id="unpaired">{identity === "gone" ? "This phone is no longer paired" : "Pair this phone first"}</h1>
      <p>
        On your computer, open Ovra, go to Settings, then Phone, and choose Add a phone. Scan the code with
        this phone's camera.
      </p>
      {#if homeScreen.standalone && homeScreen.appleMobile}
        <form class="paste" onsubmit={pairFromLink}>
          <label for="pair-link">Or paste the pairing link</label>
          <input id="pair-link" type="url" bind:value={pasted} autocomplete="off" placeholder="https://" />
          <button type="submit" class="primary">Pair</button>
          {#if pasteError}<p role="alert">{pasteError}</p>{/if}
        </form>
      {/if}
    </section>
  {:else if phone}
    {#if route.screen === "approval"}
      <Approval {phone} id={route.id} />
    {:else if route.screen === "agent"}
      <Agent {phone} session={route.session} />
    {:else if route.screen === "task"}
      <Task {phone} id={route.id} />
    {:else if route.screen === "computer"}
      <Computer {phone} name={identity.name} onForgotten={forgotten} />
    {:else}
      <Home {phone} />
    {/if}
  {/if}
</main>

<style>
  main {
    max-width: 560px;
    margin: 0 auto;
    padding: max(var(--sp-4), env(safe-area-inset-top)) var(--sp-4) max(var(--sp-6), env(safe-area-inset-bottom));
  }
  .center {
    text-align: center;
    padding-top: 20vh;
  }
  .paste {
    display: grid;
    gap: var(--sp-3);
    text-align: start;
    margin-top: var(--sp-6);
  }
  h1 {
    font: var(--t-title1);
  }
  p {
    color: var(--text-2);
  }
</style>
