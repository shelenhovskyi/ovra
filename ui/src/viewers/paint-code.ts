// Colours fenced code blocks in rendered Markdown with the editor's classes.
// The text is set through textContent, never as HTML.

import { highlightLines, languageNamed } from "../editor/highlight/static";

/** Paints each `pre > code.language-*` under `root` that is still attached when its colours arrive. */
export function paintCodeBlocks(root: HTMLElement): void {
  for (const code of root.querySelectorAll<HTMLElement>("pre > code[class^='language-']")) {
    const language = languageNamed(code.className.slice("language-".length).split(" ")[0] ?? "");
    const lines = (code.textContent ?? "").replace(/\n$/, "").split("\n");
    void highlightLines(language, lines).then((painted) => {
      if (!painted || !root.contains(code)) return;
      code.replaceChildren(
        ...painted.flatMap((segments, i) => [
          ...(i > 0 ? ["\n"] : []),
          ...segments.map(({ text, cls }) => {
            if (!cls) return text;
            const span = document.createElement("span");
            span.className = cls;
            span.textContent = text;
            return span;
          }),
        ]),
      );
    });
  }
}
