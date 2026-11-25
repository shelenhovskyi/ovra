// Keyboard moves shared by the header's menus (WAI-ARIA menu button): arrow
// keys, Home and End move between items, and a letter or digit moves to the
// next item whose name starts with it.

/**
 * The typed letter or digit, lower-case, or null. A non-Latin layout falls
 * back to the physical key, so P finds "Project policies" whatever the input language.
 */
export function typed(event: KeyboardEvent): string | null {
  if (event.ctrlKey || event.metaKey || event.altKey) return null;
  const key = (event.key ?? "").toLowerCase();
  if (/^[a-z0-9]$/.test(key)) return key;
  const physical = /^(?:Key([A-Z])|Digit(\d))$/.exec(event.code);
  return physical ? (physical[1] ?? physical[2] ?? "").toLowerCase() : null;
}

function name(item: HTMLElement): string {
  return (item.getAttribute("aria-label") ?? item.textContent ?? "").trim().toLowerCase();
}

/** The item `event` moves focus to among `items`, or null when the key is not one of the menu's. */
export function menuTarget(event: KeyboardEvent, items: HTMLElement[]): HTMLElement | null {
  const count = items.length;
  if (count === 0) return null;
  const at = items.indexOf(document.activeElement as HTMLElement);
  switch (event.code) {
    case "ArrowDown":
      return items[(at + 1) % count] ?? null;
    case "ArrowUp":
      return items[(at - 1 + count) % count] ?? null;
    case "Home":
      return items[0] ?? null;
    case "End":
      return items[count - 1] ?? null;
  }
  const letter = typed(event);
  if (!letter) return null;
  for (let step = 1; step <= count; step++) {
    const item = items[(at + step) % count];
    if (item && name(item).startsWith(letter)) return item;
  }
  return null;
}
