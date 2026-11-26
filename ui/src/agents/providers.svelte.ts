// The agent providers the runtime can start (`agents.providers`, ADR 0011),
// loaded once per transport. Places about one session or setup name its
// provider from here; generic places say "agent" or nothing.

import type { AgentKind, ProviderInfo } from "../lib/protocol";
import type { Transport } from "../lib/transport";

/** What each provider is called until `agents.providers` answers, or if it fails. */
const FALLBACK: Record<AgentKind, string> = { claude: "Claude" };

export class Providers {
  list = $state<ProviderInfo[] | null>(null);

  constructor(transport: Transport) {
    // Deferred, so a fake that throws instead of rejecting keeps the fallback too.
    void Promise.resolve()
      .then(() => transport.call({ method: "agents.providers" }))
      .then(
        ({ data }) => (this.list = data),
        () => undefined,
      );
  }

  /** The provider's display name, such as `Claude`. */
  name(id: AgentKind = "claude"): string {
    return this.list?.find((p) => p.id === id)?.name ?? FALLBACK[id] ?? id;
  }
}

const instances = new WeakMap<Transport, Providers>();

/** The providers of `transport`, asked for once. */
export function providers(transport: Transport): Providers {
  let found = instances.get(transport);
  if (!found) {
    found = new Providers(transport);
    instances.set(transport, found);
  }
  return found;
}
