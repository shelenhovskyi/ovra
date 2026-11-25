// Syntax colour for code shown outside the editor: the viewer, diffs, search
// results and Markdown code blocks. The lines go through the same worker and
// get the same `tok-*` classes (styled in shell/theme.css) as the editor.

import { type Language, languageSource, languages } from "../languages";
import { highlighter } from "./highlighter";
import type { Span } from "./protocol";

/** A run of text and its classes ("" for plain text). */
export type Segment = { text: string; cls: string };

/** Above this many characters the text stays plain, as in the editor. */
const MAX_CHARS = 1024 * 1024;

/** The language of a Markdown code block's info string (`rust`, `ts`, ...), or null. */
export function languageNamed(tag: string): Language | null {
  const name = tag.toLowerCase();
  return languages.find((l) => l.id === name || l.name.toLowerCase() === name || l.extensions.includes(name)) ?? null;
}

/**
 * `lines` highlighted as one document, cut into segments per line; null where
 * there is no language or worker (unit tests), or highlighting failed.
 */
export async function highlightLines(language: Language | null, lines: readonly string[]): Promise<Segment[][] | null> {
  const text = lines.join("\n");
  const client = highlighter();
  if (!language || !client || text.length > MAX_CHARS) return null;
  try {
    const doc = client.open(await languageSource(language), text);
    try {
      return lineSegments(text, await doc.highlight(0, text.length));
    } finally {
      doc.close();
    }
  } catch {
    return null;
  }
}

/** Cuts `text` into lines of segments; where spans nest, the innermost one's classes win. */
export function lineSegments(text: string, spans: readonly Span[]): Segment[][] {
  // Spans come sorted by start, outer before inner, so later ones paint over earlier ones.
  const owner = new Int32Array(text.length).fill(-1);
  spans.forEach(([from, to], i) => {
    owner.fill(i, Math.max(0, from), Math.min(text.length, to));
  });
  const lines: Segment[][] = [[]];
  let start = 0;
  const flush = (end: number) => {
    if (end > start) {
      const span = spans[owner[start] ?? -1];
      lines[lines.length - 1]?.push({ text: text.slice(start, end), cls: span ? span[2] : "" });
    }
    start = end;
  };
  for (let i = 0; i < text.length; i++) {
    if (text[i] === "\n") {
      flush(text[i - 1] === "\r" ? i - 1 : i);
      start = i + 1;
      lines.push([]);
    } else if (owner[i] !== owner[start]) flush(i);
  }
  flush(text.length);
  return lines;
}

/** The part of `segments` between offsets `from` and `to` of their line. */
export function sliceSegments(segments: readonly Segment[], from: number, to: number): Segment[] {
  const out: Segment[] = [];
  let at = 0;
  for (const { text, cls } of segments) {
    const end = at + text.length;
    if (end > from && at < to) out.push({ text: text.slice(Math.max(0, from - at), to - at), cls });
    at = end;
  }
  return out;
}
