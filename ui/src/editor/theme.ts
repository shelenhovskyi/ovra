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
    // The size is the person's (editor/prefs.svelte.ts), set on the editor by Editor.svelte.
    ".cm-scroller": { fontFamily: "var(--mono)", fontSize: "var(--editor-font-size, 13.5px)", lineHeight: "1.65" },
    ".cm-content": { caretColor: "var(--text)" },
    ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--text)" },
    // Line numbers keep 4.5:1 (--text-2), the active one at full strength.
    ".cm-gutters": { backgroundColor: "var(--s2)", color: "var(--text-2)", border: "none" },
    // The fold gutter after the numbers takes the rest of their old right padding.
    ".cm-lineNumbers .cm-gutterElement": { padding: "0 4px 0 16px" },
    ".cm-foldGutter .cm-gutterElement": { padding: "0 6px 0 2px", cursor: "pointer" },
    // Quiet: a fold chevron shows while the pointer is on the gutter; a folded line keeps its own.
    ".cm-foldGutter .cm-fold-open": { opacity: "0" },
    ".cm-gutters:hover .cm-fold-open": { opacity: "1" },
    ".cm-foldGutter .cm-gutterElement:hover": { color: "var(--text)" },
    ".cm-foldPlaceholder": {
      margin: "0 2px",
      padding: "0 4px",
      border: "none",
      borderRadius: "var(--radius-sm)",
      backgroundColor: "var(--s3)",
      color: "var(--text-2)",
    },
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
    // Find, Replace and Go to line: the app's 13 px secondary text, quiet
    // buttons and fields, as the Search panel has them (components.md).
    // `.cm-panels` before each part outranks CodeMirror's light and dark rules.
    ".cm-panels": { font: "var(--t-small)", backgroundColor: "var(--s1)", color: "var(--text)", userSelect: "none" },
    ".cm-panels.cm-panels-top": { borderBottom: "none" },
    ".cm-panels.cm-panels-bottom": { borderTop: "none" },
    ".cm-panels .cm-search, .cm-panels .cm-dialog": { padding: "4px 40px 4px 8px" },
    ".cm-panels .cm-search input, .cm-panels .cm-search button, .cm-panels .cm-search label": {
      margin: "2px 4px 2px 0",
      verticalAlign: "middle",
    },
    ".cm-panels .cm-search label, .cm-panels .cm-dialog label": {
      display: "inline-flex",
      alignItems: "center",
      gap: "6px",
      marginRight: "4px",
      font: "inherit",
      color: "var(--text-2)",
    },
    ".cm-panels .cm-search input[type=checkbox]": { width: "14px", height: "14px", margin: "0" },
    ".cm-panels .cm-textfield": {
      boxSizing: "border-box",
      height: "26px",
      width: "160px",
      padding: "0 8px",
      font: "inherit",
      border: "1px solid transparent",
      borderRadius: "var(--r-sm)",
      backgroundColor: "var(--s3)",
      color: "var(--text)",
      userSelect: "text",
    },
    ".cm-panels .cm-dialog .cm-textfield": { width: "88px" },
    ".cm-panels .cm-button, .cm-panels .cm-search [name=close], .cm-panels .cm-dialog-close": {
      height: "26px",
      padding: "0 8px",
      font: "inherit",
      border: "none",
      borderRadius: "var(--r-sm)",
      backgroundImage: "none",
      backgroundColor: "transparent",
      color: "var(--text-2)",
      cursor: "default",
      whiteSpace: "nowrap",
    },
    ".cm-panels .cm-button:hover, .cm-panels .cm-search [name=close]:hover, .cm-panels .cm-dialog-close:hover": {
      backgroundColor: "var(--s3)",
      color: "var(--text)",
    },
    ".cm-panels .cm-button:active": { backgroundImage: "none" },
    ".cm-panels .cm-search [name=close], .cm-panels .cm-dialog-close": {
      top: "4px",
      right: "8px",
      width: "26px",
      margin: "0",
      padding: "0",
      fontSize: "16px",
    },
  },
  { dark },
);
