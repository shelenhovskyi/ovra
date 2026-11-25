// Ovra's icons: inner SVG of a 24 x 24 view box, drawn with a 1.6 stroke in
// `currentColor` (docs/specs/quiet-style.md). `Icon.svelte` renders them.

export const ICONS = {
  files:
    '<path d="M4 6.5A1.5 1.5 0 0 1 5.5 5h4l2 2h7A1.5 1.5 0 0 1 20 8.5v9a1.5 1.5 0 0 1-1.5 1.5h-13A1.5 1.5 0 0 1 4 17.5z"/>',
  search: '<circle cx="11" cy="11" r="6"/><path d="m20 20-4.2-4.2"/>',
  branch:
    '<circle cx="7" cy="6" r="2"/><circle cx="7" cy="18" r="2"/><circle cx="17" cy="8" r="2"/><path d="M7 8v8M17 10c0 4-5 3.5-9 6.5"/>',
  agent: '<path d="M12 3v3M12 18v3M3 12h3M18 12h3"/><circle cx="12" cy="12" r="4"/>',
  terminal: '<rect x="3.5" y="5" width="17" height="14" rx="2"/><path d="m7.5 10 2.5 2-2.5 2M12.5 14.5h4"/>',
  more: '<circle cx="6" cy="12" r="1"/><circle cx="12" cy="12" r="1"/><circle cx="18" cy="12" r="1"/>',
  swap: '<path d="M4 9h15l-3.5-3.5M20 15H5l3.5 3.5"/>',
  collapse: '<rect x="3.5" y="5" width="17" height="14" rx="2"/><path d="M9.5 5v14M15.5 10l-2 2 2 2"/>',
  chevron: '<path d="m6 9 6 6 6-6"/>',
  dot: '<circle cx="12" cy="12" r="3.5" fill="currentColor"/>',
  "chevron-right": '<path d="m9 6 6 6-6 6"/>',
  close: '<path d="M6 6l12 12M18 6 6 18"/>',
  plus: '<path d="M12 5v14M5 12h14"/>',
  minus: '<path d="M5 12h14"/>',
  check: '<path d="m5 12.5 4.5 4.5L19 7.5"/>',
  external: '<path d="M14 4.5h5.5V10M19.5 4.5 11 13M17 13.5v5h-12.5v-12.5h5"/>',
  discard: '<path d="M5 9.5h9.5a4.5 4.5 0 0 1 0 9H10M5 9.5 8.5 6M5 9.5 8.5 13"/>',
  "collapse-all": '<path d="m7 15 5-5 5 5M7 9l5-5 5 5"/>',
  reveal: '<circle cx="12" cy="12" r="7"/><circle cx="12" cy="12" r="2.5"/>',
  filter: '<path d="M4 6h16M7 12h10M10 18h4"/>',
  review:
    '<path d="M5 5.5A1.5 1.5 0 0 1 6.5 4h11A1.5 1.5 0 0 1 19 5.5v13a1.5 1.5 0 0 1-1.5 1.5h-11A1.5 1.5 0 0 1 5 18.5z"/><path d="m9 12 2 2 4-4"/>',
  settings:
    '<circle cx="12" cy="12" r="3"/><path d="M12 3.5v2.2M12 18.3v2.2M3.5 12h2.2M18.3 12h2.2M6 6l1.6 1.6M16.4 16.4 18 18M6 18l1.6-1.6M16.4 7.6 18 6"/>',
  folder:
    '<path d="M4 7.5A1.5 1.5 0 0 1 5.5 6h4l2 2h7A1.5 1.5 0 0 1 20 9.5v8a1.5 1.5 0 0 1-1.5 1.5h-13A1.5 1.5 0 0 1 4 17.5z"/><path d="M12 11v5M9.5 13.5h5"/>',
} as const;

export type IconName = keyof typeof ICONS;
