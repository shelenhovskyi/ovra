// File paths in terminal output: `src/main.rs:42:7`, `./ui/a.ts(12,3)`,
// `ui/a.ts`, or an absolute path under the project root. This only finds
// candidates and turns them into project-relative paths; whether the file
// exists is checked against the project's file list by the caller.

import type { TextSpot } from "../shell/workspace.svelte";

/** A path found at UTF-16 offsets `start` to `end` (exclusive) of the text. */
export type FileLink = {
  start: number;
  end: number;
  /** Project-relative, `/`-separated, without `.` or `..` segments. */
  path: string;
  /** From 1, or null without a line suffix. */
  line: number | null;
  /** From 1, or null without a column. */
  column: number | null;
};

const CHAR = String.raw`[\p{L}\p{N}_.@+~%-]`;
const SEP = String.raw`[/\\]`;
const PATH = `${CHAR}+(?:${SEP}${CHAR}+)*`;
// `:42`, `:42:7`, `(12,3)`, `(12, 3)`, `(12)`.
const SUFFIX = String.raw`(?::(\d+)(?::(\d+))?|\((\d+)(?:,\s?(\d+))?\))`;
// Not inside a longer word, path or URL (`https://host/a.ts` has `:` or `/` before each part).
const BEFORE = String.raw`(?<![\p{L}\p{N}_.@+~%/\\:-])`;

const escapeRegExp = (text: string) => text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

function pattern(root: string | null): RegExp {
  // Without a root the first group can never match, so the groups keep their numbers.
  const rooted = root ? escapeRegExp(root.replace(/[/\\]+$/, "")) : "(?!)";
  return new RegExp(`${BEFORE}(?:(${rooted}${SEP})|(${SEP}|[A-Za-z]:${SEP}))?(${PATH})${SUFFIX}?`, "gu");
}

/** `segments` joined, with `.` and `..` resolved; null when `..` leaves the project. */
function normalize(segments: string[]): string | null {
  const out: string[] = [];
  for (const segment of segments) {
    if (segment === "" || segment === ".") continue;
    if (segment === "..") {
      if (out.pop() === undefined) return null;
    } else out.push(segment);
  }
  return out.length > 0 ? out.join("/") : null;
}

const number = (text: string | undefined) => (text === undefined ? null : Number(text));

/**
 * File paths in `text`. `root` is the project's absolute folder; relative
 * paths are taken from `base`, a project-relative folder (the shell's
 * starting folder, "" for the root). A bare word counts only with a dot, a
 * slash or a line suffix, so plain words are not links.
 */
export function findFileLinks(text: string, root: string | null, base = ""): FileLink[] {
  const links: FileLink[] = [];
  for (const match of text.matchAll(pattern(root))) {
    const [whole, inRoot, elsewhere, rawPath = ""] = match;
    // An absolute path outside the project.
    if (elsewhere !== undefined) continue;
    let raw = rawPath;
    const line = number(match[4] ?? match[6]);
    const column = number(match[5] ?? match[7]);
    let end = match.index + whole.length;
    // A sentence's full stop is not part of the name.
    if (line === null) {
      const trimmed = raw.replace(/\.+$/, "");
      end -= raw.length - trimmed.length;
      raw = trimmed;
    }
    if (inRoot === undefined && line === null && !/[./\\]/.test(raw)) continue;
    const segments = raw.split(/[/\\]/);
    const path = normalize(inRoot !== undefined ? segments : [...base.split("/"), ...segments]);
    if (!path || (line !== null && line < 1)) continue;
    links.push({ start: match.index, end, path, line, column: column !== null && column >= 1 ? column : null });
  }
  return links;
}

/** Where opening `link` puts the cursor: its line and column, as the editor's spot. */
export function linkSpot(link: FileLink): TextSpot | undefined {
  if (link.line === null) return undefined;
  const from = (link.column ?? 1) - 1;
  return { line: link.line, from, to: from };
}
