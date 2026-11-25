// Editor colours, dark only for now, including the tree-sitter capture
// classes (`tok-<capture>`, see languages/README.md).

import { EditorView } from "@codemirror/view";

export const theme = EditorView.theme(
  {
    "&": { height: "100%", color: "#e6e6e6", backgroundColor: "#16161a" },
    ".cm-scroller": { fontFamily: "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace", lineHeight: "1.5" },
    ".cm-content": { caretColor: "#e6e6e6" },
    // Line numbers at 5.3:1, the active one brighter.
    ".cm-gutters": { backgroundColor: "#16161a", color: "#8a8a96", border: "none" },
    ".cm-activeLine": { backgroundColor: "#1d1d23" },
    ".cm-activeLineGutter": { backgroundColor: "#1d1d23", color: "#d0d0d8" },
    "&.cm-focused .cm-selectionBackground, .cm-selectionBackground": { backgroundColor: "#2f3a55" },
    ".cm-selectionMatch": { backgroundColor: "#2a2f3d" },
    ".tok-comment": { color: "#6a737d", fontStyle: "italic" },
    ".tok-keyword": { color: "#c792ea" },
    ".tok-string": { color: "#c3e88d" },
    ".tok-string-special-key, .tok-property": { color: "#82aaff" },
    ".tok-escape, .tok-string-escape": { color: "#89ddff" },
    ".tok-number, .tok-constant, .tok-boolean": { color: "#f78c6c" },
    ".tok-function, .tok-constructor": { color: "#82aaff" },
    ".tok-function-macro": { color: "#89ddff" },
    ".tok-type, .tok-tag": { color: "#ffcb6b" },
    ".tok-attribute, .tok-label": { color: "#ffcb6b" },
    ".tok-variable-builtin, .tok-variable-parameter": { color: "#f07178" },
    ".tok-operator, .tok-punctuation-delimiter": { color: "#89ddff" },
    ".tok-punctuation-bracket": { color: "#a6accd" },
  },
  { dark: true },
);
