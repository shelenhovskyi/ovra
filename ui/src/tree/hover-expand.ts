// Spring-loaded folders: while dragging rows, a collapsed folder held under
// the pointer for a moment opens, so a drop can go deeper (as in VS Code).

/** How long the pointer rests on a collapsed folder before it opens. */
export const HOVER_EXPAND_MS = 600;

export class HoverExpander {
  private path: string | null = null;
  private timer: ReturnType<typeof setTimeout> | null = null;

  constructor(private readonly expand: (path: string) => void) {}

  /** On every drag over the tree: the collapsed folder under the pointer, or null. */
  over(path: string | null): void {
    if (path === this.path) return;
    this.cancel();
    this.path = path;
    if (path === null) return;
    this.timer = setTimeout(() => {
      this.timer = null;
      this.expand(path);
    }, HOVER_EXPAND_MS);
  }

  /** The drag ended or left the tree. */
  cancel(): void {
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = null;
    this.path = null;
  }
}
