// "Connect to remote machine…" in the palette and the File menu: Settings >
// Remote machines, where machines are added and opened (ADR 0016).

import { registerCommand } from "../keys/commands";
import { openSettings } from "../settings/commands";

/** Registers the command; returns a function that unregisters it. */
export function registerMachineCommands(): () => void {
  return registerCommand({
    id: "machines.connect",
    title: "Connect to remote machine…",
    category: "Remote machines",
    keywords: ["ssh", "server", "remote"],
    run: () => openSettings("machines"),
  });
}
