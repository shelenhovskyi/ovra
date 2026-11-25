// ANSI colours in server output, turned into plain text runs with a colour
// class. Only SGR colours and bold are kept; every other escape (cursor
// moves, titles, hyperlinks) is removed. The UI renders runs as text, never
// as HTML.

export type Run = {
  text: string;
  /** 0-15: the 16 terminal colours; absent: the default. */
  fg?: number;
  bg?: number;
  bold?: boolean;
};

type Style = Omit<Run, "text">;

// Text colours map to theme tokens, so both themes keep them readable; the
// bright variants look like the normal ones. Black and white follow the
// theme's text, so "white" output stays visible on a light background.
const TEXT = [
  "var(--text-2)",
  "var(--danger)",
  "var(--ok)",
  "var(--warn)",
  "var(--kw)",
  "var(--attr)",
  "var(--ty)",
  "var(--text)",
] as const;

/** Backgrounds are the soft fills, so the default text stays readable on them. */
const FILL = [
  "var(--s4)",
  "var(--danger-soft)",
  "var(--ok-soft)",
  "var(--warn-soft)",
  "var(--accent-soft)",
  "var(--accent-soft)",
  "var(--accent-soft)",
  "var(--s3)",
] as const;

/** The CSS colour of terminal colour `n` (0-15) as text. */
export const textColour = (n: number): string => TEXT[n % 8] ?? "inherit";

/** The CSS colour of terminal colour `n` (0-15) as a background. */
export const fillColour = (n: number): string => FILL[n % 8] ?? "transparent";

// CSI ... final byte, OSC ... BEL or ST, and lone two-byte escapes.
// biome-ignore lint/suspicious/noControlCharactersInRegex: matching terminal escapes is the point.
const ESCAPE = /\x1b\[([0-9;?]*)([@-~])|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)?|\x1b[@-_]?/g;
// biome-ignore lint/suspicious/noControlCharactersInRegex: other control characters are dropped.
const CONTROL = /[\x00-\x08\x0b-\x1f\x7f]/g;

/** Splits `line` into styled runs; empty runs are dropped. */
export function parseAnsi(line: string): Run[] {
  const runs: Run[] = [];
  let style: Style = {};
  let at = 0;
  const push = (text: string) => {
    const clean = text.replace(CONTROL, "");
    if (clean) runs.push({ text: clean, ...style });
  };
  for (const match of line.matchAll(ESCAPE)) {
    push(line.slice(at, match.index));
    at = match.index + match[0].length;
    if (match[2] === "m") style = applySgr(style, match[1] ?? "");
  }
  push(line.slice(at));
  return runs;
}

function applySgr(style: Style, params: string): Style {
  const codes = params === "" ? [0] : params.split(";").map((p) => Number(p) || 0);
  let next: Style = { ...style };
  for (let i = 0; i < codes.length; i++) {
    const code = codes[i] ?? 0;
    if (code === 0) next = {};
    else if (code === 1) next.bold = true;
    else if (code === 22) delete next.bold;
    else if (code >= 30 && code <= 37) next.fg = code - 30;
    else if (code >= 90 && code <= 97) next.fg = code - 90 + 8;
    else if (code === 39) delete next.fg;
    else if (code >= 40 && code <= 47) next.bg = code - 40;
    else if (code >= 100 && code <= 107) next.bg = code - 100 + 8;
    else if (code === 49) delete next.bg;
    else if (code === 38 || code === 48) {
      // 256 colours and true colour: the low 16 are kept, the rest dropped.
      const key = code === 38 ? "fg" : "bg";
      if (codes[i + 1] === 5) {
        const n = codes[i + 2] ?? 0;
        if (n < 16) next[key] = n;
        else delete next[key];
        i += 2;
      } else if (codes[i + 1] === 2) {
        delete next[key];
        i += 4;
      }
    }
  }
  return next;
}
