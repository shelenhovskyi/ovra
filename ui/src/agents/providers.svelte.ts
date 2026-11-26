// The agent providers the runtime can start (`agents.providers`, ADR 0011),
// loaded once per transport, and whether each one other than Claude is set
// up (`agents.setup`). Places about one session or setup name its provider
// from here; generic places say "agent" or nothing.

import type { AgentKind, AgentSetup, ProviderInfo } from "../lib/protocol";
import type { Transport } from "../lib/transport";

/** What each provider is called until `agents.providers` answers, or if it fails. */
const FALLBACK: Record<AgentKind, string> = { claude: "Claude", gemini: "Gemini", codex: "Codex" };

export class Providers {
  list = $state<ProviderInfo[] | null>(null);
  /** The setup of every provider but Claude, whose accounts say it. */
  setups = $state<Partial<Record<AgentKind, AgentSetup>>>({});

  constructor(private readonly transport: Transport) {
    // Deferred, so a fake that throws instead of rejecting keeps the fallback too.
    void Promise.resolve()
      .then(() => transport.call({ method: "agents.providers" }))
      .then(
        ({ data }) => {
          this.list = data;
          void this.check();
        },
        () => undefined,
      );
  }

  /** The provider's display name, such as `Claude`. */
  name(id: AgentKind = "claude"): string {
    return this.list?.find((p) => p.id === id)?.name ?? FALLBACK[id] ?? id;
  }

  /** What the provider can do, once known. */
  info(id: AgentKind): ProviderInfo | undefined {
    return this.list?.find((p) => p.id === id);
  }

  /** Claude, then every other provider that is installed and was not found signed out. */
  get startable(): AgentKind[] {
    const others = (this.list ?? [])
      .map((p) => p.id)
      .filter((id) => id !== "claude" && this.setups[id]?.state.state === "ready");
    return ["claude", ...others];
  }

  /** Checks every provider but Claude again. */
  async check(): Promise<void> {
    for (const { id } of this.list ?? []) {
      if (id === "claude") continue;
      try {
        const { data } = await this.transport.call({ method: "agents.setup", params: { agent: id } });
        this.setups = { ...this.setups, [id]: data };
      } catch {
        // Not checked: it is not offered.
      }
    }
  }

  /** Sets (or with null clears) `id`'s program, then shows its new setup. */
  async setPath(id: AgentKind, path: string | null): Promise<void> {
    const { data } = await this.transport.call({ method: "agents.set_path", params: { agent: id, path } });
    this.setups = { ...this.setups, [id]: data };
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
