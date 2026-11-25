<script lang="ts">
// Mounted once by the shell: registers the built-in commands and listens for shortcuts and the menu bar.
import { registerEditorCommands } from "../editor/commands";
import { registerFileCommands } from "../editor/file-commands";
import { registerPaletteCommands } from "../palette/commands";
import { useOvra } from "../shell/context";
import { installKeybindings, runMenuCommand } from "./keybindings";

const { transport, workspace } = useOvra();

$effect(() => {
  const unregister = registerPaletteCommands(workspace);
  const unregisterEditor = registerEditorCommands(workspace);
  const unregisterFile = registerFileCommands(workspace, transport);
  const uninstall = installKeybindings(window);
  const offMenu = transport.onMenu?.(runMenuCommand);
  return () => {
    uninstall();
    offMenu?.();
    unregister();
    unregisterEditor();
    unregisterFile();
  };
});
</script>
