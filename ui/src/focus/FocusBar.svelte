<script lang="ts">
// The title bar while Focus is on: the project, a "Focus" label and the
// way back to the IDE. Its empty space drags the window, like the IDE's.
import { formatShortcut, isApple } from "../keys/shortcut";
import { useOvra } from "../shell/context";
import { FOCUS_KEYS } from "./keys";
import { focusMode } from "./mode.svelte";

const { workspace } = useOvra();
</script>

<header class="bar" data-tauri-drag-region data-testid="focus-bar">
  <span class="project" data-tauri-drag-region>{workspace.project?.name ?? ""}</span>
  <span class="label" data-tauri-drag-region>Focus</span>
  <span class="spacer" data-tauri-drag-region></span>
  <button
    type="button"
    onclick={() => focusMode.set(false)}
    title={`Open the IDE (${formatShortcut(FOCUS_KEYS, isApple())})`}
    data-testid="focus-open-ide">Open IDE</button
  >
</header>

<style>
  .bar {
    display: flex;
    align-items: center;
    gap: 10px;
    box-sizing: border-box;
    height: 100%;
    padding: 0 8px 0 12px;
    background: var(--s0);
    user-select: none;
  }
  /* The desktop app's overlay title bar: room for the traffic lights, as in TitleBar. */
  :global([data-chrome="overlay"]) .bar {
    padding-left: 80px;
  }
  .project {
    color: var(--text-2);
  }
  .label {
    padding: 2px 10px;
    border-radius: var(--r-pill);
    background: var(--accent-soft);
    color: var(--accent);
    font-size: 12px;
  }
  .spacer {
    flex: 1;
  }
  button {
    min-height: var(--control-h-sm);
    padding: 0 12px;
    color: var(--text);
    background: var(--s3);
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    white-space: nowrap;
  }
  button:hover {
    background: var(--s4);
  }
</style>
