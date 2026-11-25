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

/** The 16 terminal colours, readable on the log's dark background. */
export const PALETTE = [
  "#6e6e7a",
  "#ff7b72",
  "#7ee787",
  "#e3b341",
  "#79c0ff",
  "#d2a8ff",
  "#56d4dd",
  "#c9d1d9",
  "#8b949e",
  "#ffa198",
  "#a5f3a5",
  "#f2cc60",
  "#a5d6ff",
  "#e2c5ff",
  "#b3f0ff",
  "#ffffff",
] as const;

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
