// Mirrors crates/core/src/protocol/power.rs.

/**
 * When Ovra keeps the computer from sleeping while idle. The display may
 * still sleep; closing a laptop's lid may still put it to sleep.
 */
export type KeepAwakeMode = "off" | "while_agents_work" | "always";

export type PowerState = {
  mode: KeepAwakeMode;
  /** Whether Ovra keeps the computer awake right now. */
  awake: boolean;
  /** Why the computer cannot be kept awake; the mode stays as chosen. */
  problem?: string;
};

export type PowerRequest =
  | { method: "power.get" }
  /** Changes the mode for this run of Ovra; every start is `off`. */
  | { method: "power.set_mode"; params: { mode: KeepAwakeMode } };

export type PowerResponse = { type: "power.state"; data: PowerState };

export type PowerEvent = { type: "power.changed"; data: PowerState };
