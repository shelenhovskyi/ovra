<script lang="ts">
// Test-only host: Reviews and its title-bar button with the shell's context and key handling.
import Keybindings from "../keys/Keybindings.svelte";
import type { Transport } from "../lib/transport";
import { provideOvra } from "../shell/context";
import type { Workspace } from "../shell/workspace.svelte";
import Inbox from "./Inbox.svelte";
import InboxButton from "./InboxButton.svelte";
import { ReviewInbox } from "./reviews.svelte";

let { transport, workspace }: { transport: Transport; workspace: Workspace } = $props();

// svelte-ignore state_referenced_locally
provideOvra({ transport, workspace });
// svelte-ignore state_referenced_locally
const inbox = new ReviewInbox(transport);
$effect(() => () => inbox.dispose());
</script>

<Keybindings />
<button type="button" data-testid="elsewhere">Elsewhere</button>
<InboxButton {inbox} />
<Inbox {inbox} />
