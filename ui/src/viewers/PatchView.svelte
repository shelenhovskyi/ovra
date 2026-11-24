<script lang="ts">
import { parsePatch } from "./patch";

let { text }: { text: string } = $props();

const files = $derived(parsePatch(text));
</script>

<div class="patch" data-testid="patch-view">
  {#each files as file, f (f)}
    <section>
      <h2>
        {#if file.oldPath === null}
          <span class="badge added">new</span> {file.newPath}
        {:else if file.newPath === null}
          <span class="badge removed">deleted</span> {file.oldPath}
        {:else if file.oldPath !== file.newPath}
          {file.oldPath} → {file.newPath}
        {:else}
          {file.newPath}
        {/if}
      </h2>
      <table>
        <tbody>
          {#each file.hunks as hunk, h (h)}
            <tr class="hunk"><td colspan="3">{hunk.header}</td></tr>
            {#each hunk.lines as line, l (l)}
              <tr class={line.kind}>
                <td class="no">{"old" in line ? line.old : ""}</td>
                <td class="no">{"new" in line ? line.new : ""}</td>
                <td class="code">{line.kind === "added" ? "+" : line.kind === "removed" ? "-" : " "}{line.text}</td>
              </tr>
            {/each}
          {/each}
        </tbody>
      </table>
    </section>
  {:else}
    <pre class="raw">{text}</pre>
  {/each}
</div>

<style>
  .patch {
    padding: 8px 12px;
    font-family: ui-monospace, monospace;
    font-size: 12px;
  }
  h2 {
    margin: 12px 0 4px;
    font-size: 13px;
    font-weight: 600;
  }
  table {
    width: 100%;
    border-collapse: collapse;
  }
  td {
    padding: 0 6px;
    white-space: pre;
  }
  .no {
    width: 1%;
    text-align: right;
    color: #6f6f7a;
    user-select: none;
  }
  .hunk td {
    color: #8a8ad6;
    background: #1e1e2a;
  }
  .added {
    background: #16301e;
  }
  .removed {
    background: #3a1a1e;
  }
  .note td {
    color: #6f6f7a;
  }
  .badge {
    padding: 0 4px;
    border-radius: 3px;
    font-size: 11px;
  }
  .raw {
    margin: 0;
    white-space: pre-wrap;
  }
</style>
