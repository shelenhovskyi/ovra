// Whether Ovra keeps the computer awake (`power.*`): the core's state, kept
// current from `power.changed`, and a way to change the mode.
import type { KeepAwakeMode, PowerState } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";

export class KeepAwake {
  /** `null` until the core answered, or when this front end has no power service. */
  state = $state<PowerState | null>(null);
  error = $state<string | null>(null);

  constructor(private readonly transport: Transport) {}

  /** Loads the state and follows its changes; returns the unsubscribe. */
  follow(): () => void {
    const off = this.transport.onEvent((event) => {
      if (event.type === "power.changed") this.state = event.data;
    });
    this.transport
      .call({ method: "power.get" })
      .then((reply) => {
        this.state = reply.data;
      })
      .catch(() => {
        // A front end without the power service shows no switch.
      });
    return off;
  }

  async set(mode: KeepAwakeMode): Promise<void> {
    this.error = null;
    try {
      this.state = (await this.transport.call({ method: "power.set_mode", params: { mode } })).data;
    } catch (err) {
      this.error = err instanceof CoreError ? err.error.message : String(err);
    }
  }

  /** The header switch: on means "while agents work", off means off. */
  toggle(): Promise<void> {
    return this.set(this.state && this.state.mode !== "off" ? "off" : "while_agents_work");
  }
}

/** One line for a tooltip or a hint. */
export function describe(state: PowerState): string {
  if (state.problem) return `Can't keep the computer awake: ${state.problem}`;
  if (state.mode === "off") return "The computer may sleep as usual";
  if (state.awake) return "Keeping the computer awake. The display may still turn off";
  return "Keeps the computer awake while agents work";
}
