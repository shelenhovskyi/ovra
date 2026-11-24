// Shared handles every panel gets from the shell: the transport to the core
// and the workspace state. Panels call `useOvra()` instead of importing globals,
// so tests can mount a panel with a fake transport.

import { getContext, setContext } from "svelte";
import type { Transport } from "../lib/transport";
import type { Workspace } from "./workspace.svelte";

export type Ovra = { transport: Transport; workspace: Workspace };

const KEY = Symbol("ovra");

export function provideOvra(ovra: Ovra): void {
  setContext(KEY, ovra);
}

export function useOvra(): Ovra {
  const ovra = getContext<Ovra | undefined>(KEY);
  if (!ovra) throw new Error("useOvra() called outside the Ovra shell");
  return ovra;
}
