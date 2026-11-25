<script lang="ts">
// The title bar's Reviews button: how many reviews wait, and that an agent
// asked for one. It opens and closes the Reviews view (the inbox.toggle
// command, which the view registers).
import { listCommands } from "../keys/commands";
import { formatShortcut, inBrowserTab, isApple, liveKeys } from "../keys/shortcut";
import Icon from "../shell/Icon.svelte";
import { mainView } from "../shell/main-view.svelte";
import { REVIEWS_KEYS } from "./keys";
import type { ReviewInbox } from "./reviews.svelte";

let { inbox }: { inbox: ReviewInbox } = $props();

const shortcut = formatShortcut(liveKeys(REVIEWS_KEYS, inBrowserTab()), isApple());

function toggle(): void {
  void listCommands()
    .find((c) => c.id === "inbox.toggle")
    ?.run();
}
</script>

<!-- Styled by the title bar it sits in (`rail-button`, `tip`). -->
<button
  type="button"
  class="rail-button tip open-inbox"
  class:asked={inbox.asked}
  data-testid="inbox-button"
  data-tip={`Reviews ${shortcut}`}
  aria-label={inbox.waiting.length > 0 ? `Reviews, ${inbox.waiting.length} waiting` : "Reviews"}
  aria-keyshortcuts={liveKeys(REVIEWS_KEYS, inBrowserTab()).replace("Mod", isApple() ? "Meta" : "Control")}
  aria-pressed={mainView.current === "reviews"}
  onclick={toggle}
>
  <Icon name="review" />
  {#if inbox.waiting.length > 0}<span class="rail-badge" aria-hidden="true">{inbox.waiting.length}</span>{/if}
</button>
<span class="visually-hidden" aria-live="polite" data-testid="inbox-asked"
  >{inbox.asked ? `An agent asks you to review its changes. Open Reviews with ${shortcut}.` : ""}</span
>

<style>
  /* Quiet until an agent asks. */
  .open-inbox.asked {
    color: var(--text);
    background: var(--warn-soft);
  }
  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
</style>
