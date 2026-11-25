// The window's layout: how wide the two side panels are (the rail's panel and
// Agents), whether each is folded to a strip, and which side each sits on.
// Kept in the page's storage like the last project; a broken or blocked
// storage only costs the restore.

export type Panel = "side" | "agents";

export const MIN_WIDTH = 180;
export const MAX_WIDTH = 460;
export const DEFAULT_WIDTH: Record<Panel, number> = { side: 260, agents: 320 };

const KEY = "ovra.layout";

type Saved = { side: PanelState; agents: PanelState; swapped: boolean };
type PanelState = { width: number; collapsed: boolean };

function storage(): Storage | null {
  try {
    return typeof localStorage === "undefined" ? null : localStorage;
  } catch {
    return null;
  }
}

function clampWidth(width: number): number {
  return Math.round(Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, width)));
}

function readPanel(value: unknown, panel: Panel): PanelState {
  const saved = (typeof value === "object" && value !== null ? value : {}) as Partial<PanelState>;
  return {
    width:
      typeof saved.width === "number" && Number.isFinite(saved.width) ? clampWidth(saved.width) : DEFAULT_WIDTH[panel],
    collapsed: saved.collapsed === true,
  };
}

export class Layout {
  side = $state<PanelState>({ width: DEFAULT_WIDTH.side, collapsed: false });
  agents = $state<PanelState>({ width: DEFAULT_WIDTH.agents, collapsed: false });
  /** Agents on the left, the rail's panel on the right. */
  swapped = $state(false);
  readonly #store: Storage | null;

  constructor(store = storage()) {
    this.#store = store;
    let saved: Partial<Saved> = {};
    try {
      saved = JSON.parse(store?.getItem(KEY) ?? "{}") ?? {};
    } catch {
      // Unreadable: start from the defaults.
    }
    this.side = readPanel(saved.side, "side");
    this.agents = readPanel(saved.agents, "agents");
    this.swapped = saved.swapped === true;
  }

  /** The panel on the left of the main area. */
  get left(): Panel {
    return this.swapped ? "agents" : "side";
  }

  resize(panel: Panel, width: number): void {
    this[panel].width = clampWidth(width);
    this.#save();
  }

  reset(panel: Panel): void {
    this.resize(panel, DEFAULT_WIDTH[panel]);
  }

  setCollapsed(panel: Panel, collapsed: boolean): void {
    this[panel].collapsed = collapsed;
    this.#save();
  }

  swap(): void {
    this.swapped = !this.swapped;
    this.#save();
  }

  #save(): void {
    try {
      const saved: Saved = { side: { ...this.side }, agents: { ...this.agents }, swapped: this.swapped };
      this.#store?.setItem(KEY, JSON.stringify(saved));
    } catch {
      // A full or blocked storage only costs the restore.
    }
  }
}
