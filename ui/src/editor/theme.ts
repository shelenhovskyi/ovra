// Editor colours from the theme tokens (ui/src/shell/theme.css), so the dark
// and light themes both follow the system. The tree-sitter capture classes
// (`tok-<capture>`) are styled in theme.css, shared with the viewers.

import { EditorView } from "@codemirror/view";

/** Picks CodeMirror's own dark or light defaults for the parts not styled here. */
const dark = typeof matchMedia !== "function" || !matchMedia("(prefers-color-scheme: light)").matches;

const tint = (percent: number) => `color-mix(in srgb, var(--accent) ${percent}%, transparent)`;

export const theme = EditorView.theme(
  {
    "&": { height: "100%", color: "var(--text)", backgroundColor: "var(--s2)" },
    // The panel shows focus; CodeMirror's dotted outline would draw a hairline.
    "&.cm-focused": { outline: "none" },
    ".cm-scroller": { fontFamily: "var(--mono)", fontSize: "13.5px", lineHeight: "1.65" },
    ".cm-content": { caretColor: "var(--text)" },
    ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--text)" },
    // Line numbers keep 4.5:1 (--text-2), the active one at full strength.
    ".cm-gutters": { backgroundColor: "var(--s2)", color: "var(--text-2)", border: "none" },
    ".cm-lineNumbers .cm-gutterElement": { padding: "0 12px 0 16px" },
    ".cm-activeLine": { backgroundColor: tint(7) },
    ".cm-activeLineGutter": { backgroundColor: tint(7), color: "var(--text)" },
    "&.cm-focused .cm-selectionBackground, .cm-selectionBackground": { backgroundColor: tint(26) },
    ".cm-selectionMatch": { backgroundColor: tint(14) },
    "&.cm-focused .cm-matchingBracket": { backgroundColor: tint(22), outline: "none" },
    ".cm-searchMatch": { backgroundColor: "var(--warn-soft)", outline: "none" },
    ".cm-searchMatch.cm-searchMatch-selected": { backgroundColor: tint(30) },
    ".cm-tooltip": {
      border: "none",
      borderRadius: "var(--radius-sm)",
      backgroundColor: "var(--s4)",
      color: "var(--text)",
      boxShadow: "var(--card-shadow)",
    },
    ".cm-tooltip-autocomplete ul li[aria-selected]": { backgroundColor: tint(26), color: "var(--text)" },
    ".cm-panels": { backgroundColor: "var(--s1)", color: "var(--text)" },
    ".cm-panels.cm-panels-top": { borderBottom: "none" },
    ".cm-panels.cm-panels-bottom": { borderTop: "none" },
    ".cm-textfield": {
      border: "1px solid transparent",
      borderRadius: "var(--radius-sm)",
      backgroundColor: "var(--s3)",
      color: "var(--text)",
    },
    ".cm-button": {
      border: "1px solid transparent",
      borderRadius: "var(--radius-sm)",
      backgroundImage: "none",
      backgroundColor: "var(--s3)",
      color: "var(--text)",
    },
  },
  { dark },
);
