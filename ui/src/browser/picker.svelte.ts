// Pick mode of one page: which element the pointer is over, asked of the core
// with `browser.pick` at most once per `interval` ms, and how many levels up
// from it the person moved with Arrow Up and Down.

import type { PageId, Pick } from "../lib/protocol";
import type { Transport } from "../lib/transport";

type Point = { x: number; y: number };

/** At most this many hit tests a second while the pointer moves. */
const HOVER_INTERVAL_MS = 100;

export class Picker {
  active = $state(false);
  /** The element the overlay shows, `up` levels above the one under the pointer. */
  hovered = $state<Pick | null>(null);
  up = $state(0);
  private at: Point | null = null;
  private asked: { at: Point; up: number } | null = null;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private moved = false;
  private seq = 0;

  constructor(
    private readonly transport: Transport,
    private readonly page: PageId,
  ) {}

  /** The page point pick mode looks at: the pointer's, or where the keyboard started. */
  get point(): Point | null {
    return this.at;
  }

  start(): void {
    this.active = true;
  }

  stop(): void {
    this.active = false;
    this.hovered = null;
    this.up = 0;
    this.at = null;
    this.asked = null;
    this.moved = false;
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
    this.seq++;
  }

  /** The pointer is over page point `at`. */
  hover(at: Point): void {
    if (!this.active) return;
    if (this.at?.x !== at.x || this.at?.y !== at.y) this.up = 0;
    this.at = at;
    if (this.timer) {
      this.moved = true;
      return;
    }
    void this.ask();
    this.timer = setTimeout(() => {
      this.timer = null;
      if (this.moved && this.at) {
        this.moved = false;
        this.hover(this.at);
      }
    }, HOVER_INTERVAL_MS);
  }

  /** Arrow Up: the parent of the shown element. */
  parent(): void {
    if (!this.active || !this.at) return;
    this.up++;
    void this.ask();
  }

  /** Arrow Down: back towards the element under the pointer. */
  child(): void {
    if (!this.active || !this.at || this.up === 0) return;
    this.up--;
    void this.ask();
  }

  /** The element to pick at `at`: the shown one if it is for that point, else asked now. */
  async take(at: Point): Promise<Pick | null> {
    if (!this.active) return null;
    const asked = this.asked;
    if (this.hovered && asked && asked.at.x === at.x && asked.at.y === at.y && asked.up === this.up)
      return this.hovered;
    this.at = at;
    return this.ask();
  }

  private async ask(): Promise<Pick | null> {
    const at = this.at;
    if (!at) return null;
    const mine = ++this.seq;
    const up = this.up;
    try {
      const { data } = await this.transport.call({
        method: "browser.pick",
        params: { page: this.page, x: at.x, y: at.y, up },
      });
      // A later question or leaving pick mode makes this answer stale.
      if (mine !== this.seq) return data;
      // Above the root the core answers the root again: stay at its level.
      if (this.asked && up > this.asked.up && data.selector === this.hovered?.selector) {
        this.up = this.asked.up;
        return data;
      }
      this.hovered = data;
      this.asked = { at, up };
      return data;
    } catch {
      if (mine === this.seq) this.hovered = null;
      return null;
    }
  }
}
