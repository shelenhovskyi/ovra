<script lang="ts">
// The title bar while Focus is on: the project, a quiet "Focus" word and
// Exit Focus, the way back to files and code (a label, never a second
// button-like pill). Its empty space drags the window, like the IDE's.
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
    title={`Show files and code again (${formatShortcut(FOCUS_KEYS, isApple())})`}
    aria-keyshortcuts={FOCUS_KEYS}
    data-testid="focus-open-ide">Exit Focus</button
  >
</header>

<style>
  .bar {
    display: flex;
    align-items: center;
    gap: 10px;
    box-sizing: border-box;
    height: 100%;
    padding: 0 12px;
    background: var(--s0);
    user-select: none;
  }
  /* The desktop app's overlay title bar: room for the traffic lights, as in TitleBar. */
  :global([data-chrome="overlay"]) .bar {
    padding-left: 88px;
  }
  .project {
    color: var(--text-2);
  }
  .label {
    color: var(--text-2);
  }
  .label::before {
    content: "·";
    margin-right: 10px;
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
    border-radius: var(--r-control);
    white-space: nowrap;
  }
  button:hover {
    background: var(--s4);
  }
</style>
