<script lang="ts">
// Shown while the core polls folders because the OS ran out of file
// watches (`fs.watch_limited`). Dismissing hides it until the count grows.
let { unwatched }: { unwatched: number } = $props();

const SETTING = "fs.inotify.max_user_watches=524288";
let dismissed = $state<number | null>(null);
</script>

{#if unwatched > (dismissed ?? 0)}
  <div class="notice" role="status" data-testid="watch-limit-notice">
    <p>
      {unwatched === 1 ? "1 folder is" : `${unwatched} folders are`} not watched for changes: the system limit on file
      watches is reached. Ovra checks them every few seconds instead, so changes there show up late.
    </p>
    <p>
      On Linux, raise the limit with <code>sudo sysctl {SETTING}</code>. To keep it after a restart, add
      <code>{SETTING}</code> to <code>/etc/sysctl.d/99-inotify.conf</code>.
    </p>
    <button type="button" onclick={() => (dismissed = unwatched)}>Dismiss</button>
  </div>
{/if}

<style>
  .notice {
    padding: 6px 8px;
    border-bottom: 1px solid #2a2a31;
    background: #2a2618;
    color: #e2c08d;
    font-size: 12px;
  }
  p {
    margin: 0 0 4px;
  }
  code {
    user-select: all;
    color: #f0e0b8;
  }
  button {
    padding: 2px 6px;
    border: 1px solid #4a4330;
    border-radius: 3px;
    background: #1b1b20;
    color: #c8c8d0;
    font: inherit;
  }
</style>
