// The window's layout: how wide the two side panels are (the rail's panel and
// Agents), whether each is folded to a strip, and which side each sits on;
// the share of the main area the left editor group takes when split; and the
// height of the bottom dock (terminals, servers, problems).
// Kept in the page's storage like the last project; a broken or blocked
// storage only costs the restore.

export type Panel = "side" | "agents";

export const MIN_WIDTH = 160;
/** A side panel takes at most this share of the window... */
export const MAX_SHARE = 0.6;
/** ...and always leaves the main area this wide. */
export const MIN_MAIN = 320;
export const DEFAULT_WIDTH: Record<Panel, number> = { side: 260, agents: 320 };
/** What sits beside the panels: the rail, the two handles, a folded Agents strip. */
const RAIL = 52;
const HANDLES = 16;
const STRIP = 48;

const KEY = "ovra.layout";

/** The bottom dock is at least this tall... */
export const MIN_DOCK = 120;
/** ...and leaves the files above it this much of the main area. */
export const MIN_ABOVE_DOCK = 160;

/** The left editor group's share of the main area stays within these. */
const MIN_GROUPS = 0.1;
const MAX_GROUPS = 0.9;

type Saved = { side: PanelState; agents: PanelState; swapped: boolean; groups?: number; dock?: number };
type PanelState = { width: number; collapsed: boolean };

function storage(): Storage | null {
  try {
    return typeof localStorage === "undefined" ? null : localStorage;
  } catch {
    return null;
  }
}

function clampWidth(width: number, max = Number.POSITIVE_INFINITY): number {
  return Math.round(Math.min(max, Math.max(MIN_WIDTH, width)));
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
  /** The left editor group's share of the main area while it is split. */
  groups = $state(0.5);
  /** The bottom dock's height in pixels; null for the default share of the main area. */
  dock = $state<number | null>(null);
  /** The window's width, kept up to date by the shell: it bounds the panels. */
  viewport = $state(typeof window === "undefined" ? 1440 : window.innerWidth);
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
    if (typeof saved.groups === "number" && Number.isFinite(saved.groups)) this.resizeGroups(saved.groups, false);
    if (typeof saved.dock === "number" && Number.isFinite(saved.dock))
      this.dock = Math.max(MIN_DOCK, Math.round(saved.dock));
  }

  /** The panel on the left of the main area. */
  get left(): Panel {
    return this.swapped ? "agents" : "side";
  }

  /** The widest `panel` may be now: a share of the window that leaves the main area its room. */
  max(panel: Panel): number {
    const other = this[panel === "side" ? "agents" : "side"];
    const beside = other.collapsed ? (panel === "side" ? STRIP : 0) : other.width;
    const room = this.viewport - RAIL - HANDLES - MIN_MAIN - beside;
    return Math.max(MIN_WIDTH, Math.floor(Math.min(this.viewport * MAX_SHARE, room)));
  }

  /** The width `panel` is drawn at: the remembered one, within what the window allows now. */
  width(panel: Panel): number {
    return Math.min(this[panel].width, this.max(panel));
  }

  resize(panel: Panel, width: number): void {
    this[panel].width = clampWidth(width, this.max(panel));
    this.#save();
  }

  reset(panel: Panel): void {
    this.resize(panel, DEFAULT_WIDTH[panel]);
  }

  /** Default widths, both panels shown, files on the left. */
  resetAll(): void {
    this.side = { width: DEFAULT_WIDTH.side, collapsed: false };
    this.agents = { width: DEFAULT_WIDTH.agents, collapsed: false };
    this.swapped = false;
    this.groups = 0.5;
    this.dock = null;
    this.#save();
  }

  /** The tallest the dock may be in a main area `room` pixels tall. */
  dockMax(room: number): number {
    return Math.max(MIN_DOCK, Math.floor(room - MIN_ABOVE_DOCK));
  }

  /**
   * The dock's height in a main area `room` pixels tall; null for the default
   * share. Not clamped while `room` is not measured yet (null), so the first
   * frame does not open the dock's terminals at the minimum and resize them.
   */
  dockHeight(room: number | null): number | null {
    if (this.dock === null || room === null) return this.dock;
    return Math.min(this.dock, this.dockMax(room));
  }

  /** Sets the dock's height within what a main area `room` pixels tall allows; null goes back to the default. */
  resizeDock(height: number | null, room: number): void {
    this.dock = height === null ? null : Math.round(Math.min(this.dockMax(room), Math.max(MIN_DOCK, height)));
    this.#save();
  }

  /** Gives the left editor group `share` of the main area. */
  resizeGroups(share: number, save = true): void {
    this.groups = Math.min(MAX_GROUPS, Math.max(MIN_GROUPS, share));
    if (save) this.#save();
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
      const saved: Saved = {
        side: { ...this.side },
        agents: { ...this.agents },
        swapped: this.swapped,
        groups: this.groups,
        ...(this.dock === null ? {} : { dock: this.dock }),
      };
      this.#store?.setItem(KEY, JSON.stringify(saved));
    } catch {
      // A full or blocked storage only costs the restore.
    }
  }
}
