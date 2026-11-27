// Brackets as other editors have them: the pair of the bracket at the cursor
// is marked, typing an opening bracket or quote adds its closing one (off in
// Settings > Editor), and Backspace between an empty pair removes both.
// Ovra has no CodeMirror language (highlighting is tree-sitter), so matching
// scans the text, and the pairs each language closes are given here.

import { closeBrackets, closeBracketsKeymap } from "@codemirror/autocomplete";
import { bracketMatching } from "@codemirror/language";
import { EditorState, type Extension, Prec } from "@codemirror/state";
import { keymap } from "@codemirror/view";
import type { Language } from "./languages";

const DEFAULT_PAIRS = ["(", "[", "{", "'", '"'];

/**
 * What typing closes, by language id. Rust has lifetimes (`'a`), so a
 * single quote stays single there; JavaScript and TypeScript close backticks.
 */
const PAIRS: Record<string, string[]> = {
  css: DEFAULT_PAIRS,
  javascript: [...DEFAULT_PAIRS, "`"],
  json: ["(", "[", "{", '"'],
  python: DEFAULT_PAIRS,
  rust: ["(", "[", "{", '"'],
  tsx: [...DEFAULT_PAIRS, "`"],
  typescript: [...DEFAULT_PAIRS, "`"],
};

/** The pairs typing closes in `language`; plain text closes brackets and double quotes only. */
export function closingPairs(language: Language | null): string[] {
  return (language && PAIRS[language.id]) ?? ["(", "[", "{", '"'];
}

/** Marks the bracket paired with the one at the cursor, always on. */
export function bracketMarks(): Extension {
  return bracketMatching();
}

/** Typing an opening bracket adds its closing one, for `language`'s pairs. */
export function autoClose(language: Language | null): Extension {
  return [
    EditorState.languageData.of(() => [{ closeBrackets: { brackets: closingPairs(language) } }]),
    closeBrackets(),
    // Before the editor's own Backspace, which would remove only the opening one.
    Prec.high(keymap.of(closeBracketsKeymap)),
  ];
}
