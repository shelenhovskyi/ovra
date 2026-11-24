// Windowing for long line lists: only the rows on screen are in the DOM.

/** Lists up to this many rows are rendered in full, so selecting and copying all of it works. */
export const RENDER_ALL_BELOW = 2000;

export function visibleRange(
  count: number,
  scrollTop: number,
  height: number,
  rowHeight: number,
  overscan = 30,
): { start: number; end: number } {
  if (count <= RENDER_ALL_BELOW) return { start: 0, end: count };
  const start = Math.max(0, Math.min(count, Math.floor(scrollTop / rowHeight) - overscan));
  const end = Math.min(count, Math.ceil((scrollTop + Math.max(height, rowHeight)) / rowHeight) + overscan);
  return { start, end: Math.max(start, end) };
}
