<script lang="ts">
// Mounted once by the shell: registers the built-in commands and listens for shortcuts.
import { registerPaletteCommands } from "../palette/commands";
import { useOvra } from "../shell/context";
import { installKeybindings } from "./keybindings";

const { workspace } = useOvra();

$effect(() => {
  const unregister = registerPaletteCommands(workspace);
  const uninstall = installKeybindings(window);
  return () => {
    uninstall();
    unregister();
  };
});
</script>
