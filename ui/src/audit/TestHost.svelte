<script lang="ts">
// Test-only host: the audit log view with the shell's context, opened as the shell opens it.
import type { Transport } from "../lib/transport";
import { provideOvra } from "../shell/context";
import { mainView } from "../shell/main-view.svelte";
import type { Workspace } from "../shell/workspace.svelte";
import AuditLog from "./AuditLog.svelte";

let { transport, workspace }: { transport: Transport; workspace: Workspace } = $props();

// svelte-ignore state_referenced_locally
provideOvra({ transport, workspace });
</script>

<button type="button" data-testid="open-audit" onclick={() => mainView.toggle("audit")}>Audit log</button>
{#if mainView.current === "audit"}<AuditLog />{/if}
