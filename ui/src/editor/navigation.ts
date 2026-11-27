// Go back and Go forward: the places a person jumped between, as in VS Code.
// A place is a file in a mode and, in the editor, the cursor. Moving to
// another file or far within one (Go to line, a search result, a definition)
// adds a place; small moves and typing only update the current one, so
// going back returns to where the cursor was last.

import type { MainMode } from "../shell/workspace.svelte";

export type Place = {
  path: string;
  mode: MainMode;
  /** The cursor's line, from 1; unknown in the viewer and until the editor reports it. */
  line?: number;
  /** The cursor's UTF-16 offset in its line, from 0. */
  column?: number;
};

/** A cursor move of more lines than this is a jump of its own. */
export const JUMP_LINES = 10;
/** Places kept per project; the oldest go first. */
const HISTORY_MAX = 50;

const far = (a: number | undefined, b: number | undefined) =>
  a !== undefined && b !== undefined && Math.abs(a - b) > JUMP_LINES;

/** The known parts of `next` over `place`. */
function merged(place: Place, next: Place): Place {
  return {
    path: place.path,
    mode: next.mode,
    line: next.line ?? place.line,
    column: next.line === undefined ? place.column : next.column,
  };
}

export class NavigationHistory {
  #places: Place[] = [];
  #index = -1;
  /** The place `back` or `forward` went to, until the window shows it. */
  #arriving: Place | null = null;

  constructor(readonly max = HISTORY_MAX) {}

  get current(): Place | null {
    return this.#places[this.#index] ?? null;
  }

  get places(): readonly Place[] {
    return this.#places;
  }

  /**
   * The window shows `place`: a new place when it is another file or a
   * jump far from the current cursor; otherwise the current place moves.
   * An `edited` move (typing, undo, a paste) is never a jump.
   */
  visit(place: Place, edited = false): void {
    const current = this.current;
    const arriving = this.#arriving;
    if (arriving && current && place.path === arriving.path) {
      // Showing a place gone back to: its file opens, then the cursor lands.
      this.#places[this.#index] = merged(current, place);
      if (arriving.line === undefined || (place.line !== undefined && !far(place.line, arriving.line)))
        this.#arriving = null;
      return;
    }
    this.#arriving = null;
    if (current && current.path === place.path && (edited || !far(place.line, current.line))) {
      this.#places[this.#index] = merged(current, place);
      return;
    }
    this.#places.splice(this.#index + 1);
    this.#places.push({ ...place });
    if (this.#places.length > this.max) this.#places.shift();
    this.#index = this.#places.length - 1;
  }

  /**
   * The window is going to show `place`, a spot asked for (a search result,
   * a definition, a line in Go to file): a visit, and until the cursor
   * gets there, the cursor the file shows first is not a jump of its own.
   */
  arrive(place: Place): void {
    this.visit(place);
    if (place.line !== undefined) this.#arriving = { ...place };
  }

  /** The place before the current one, now current; null at the oldest. */
  back(): Place | null {
    return this.#go(-1);
  }

  /** The place after the current one, now current; null at the newest. */
  forward(): Place | null {
    return this.#go(1);
  }

  #go(step: -1 | 1): Place | null {
    const from = this.current;
    const to = this.#places[this.#index + step];
    if (!from || !to) return null;
    this.#index += step;
    // Within the file shown, a place without a cursor shows nothing new.
    this.#arriving = to.path === from.path && to.line === undefined ? null : to;
    return { ...to };
  }
}
