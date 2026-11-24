# Persona: Iryna, who relies on accessibility support

**Who:** an experienced developer with low vision. Uses a screen reader (NVDA
on Windows, VoiceOver on macOS), 200% zoom, and sometimes only the keyboard
because of RSI.

**Goal in a session:** do everything the others do, with the same speed, using
a screen reader, zoom and keyboard.

**Habits and expectations**

- Navigates by landmarks and headings; expects names, roles and states on every
  control (WAI-ARIA tree, combobox, listbox, dialog patterns).
- Needs visible focus everywhere, contrast of at least 4.5:1 for text, and
  layouts that survive 200% zoom and a 1024 px wide window.
- Needs status changes (loading, errors, connected) announced via live regions.
- Avoids hover-only information and colour-only signals (git status as colour).

**What she checks**

- Accessibility tree (`page.accessibility.snapshot()` or `ariaSnapshot()`) of
  each screen: landmarks, headings, names, roles, states.
- Contrast of text and focus indicators (compute from computed styles).
- Zoom 200% and narrow viewport: nothing cut off or overlapping.
- `prefers-reduced-motion`, `forced-colors`, and colour-only signals.

**Fixtures:** `small`.
