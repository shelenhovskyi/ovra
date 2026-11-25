<script lang="ts">
// Shown while the core polls folders because the OS ran out of file
// watches (`fs.watch_limited`). Dismissing hides it until the count grows.
let { unwatched }: { unwatched: number } = $props();

const SETTING = "fs.inotify.max_user_watches=524288";
let dismissed = $state<number | null>(null);
</script>

{#if unwatched > (dismissed ?? 0)}
  <div class="notice" role="status" data-testid="watch-limit-notice">
    <p>{unwatched === 1 ? "1 folder is" : `${unwatched} folders are`} not watched; changes show up late.</p>
    <details>
      <summary>Details</summary>
      <p>
        The system limit on file watches is reached, so Ovra checks these folders every few seconds. On Linux, run
        <code>sudo sysctl {SETTING}</code>; to keep it, add <code>{SETTING}</code> to
        <code>/etc/sysctl.d/99-inotify.conf</code>.
      </p>
    </details>
    <button type="button" onclick={() => (dismissed = unwatched)}>Dismiss</button>
  </div>
{/if}

<style>
  .notice {
    margin: 0 8px 8px;
    padding: 8px 10px;
    border-radius: var(--radius);
    background: var(--warn-soft);
    color: var(--text);
    font-size: 13px;
  }
  p {
    margin: 0 0 6px;
  }
  summary {
    color: var(--text-2);
    cursor: pointer;
  }
  details p {
    margin-top: 4px;
    color: var(--text-2);
  }
  code {
    user-select: all;
    color: var(--text);
    overflow-wrap: anywhere;
  }
  button {
    margin-top: 4px;
    padding: 2px 8px;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--text-2);
    font: inherit;
  }
  button:hover {
    background: var(--s3);
    color: var(--text);
  }
</style>
