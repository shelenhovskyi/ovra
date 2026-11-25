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
} as const;

export type IconName = keyof typeof ICONS;
