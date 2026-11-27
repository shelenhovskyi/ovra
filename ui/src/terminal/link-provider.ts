// Turns file paths in a terminal's output into links (xterm.js
// `registerLinkProvider`). The parsing is in file-links.ts; this reads the
// buffer, keeps only files the project has, and opens them on Mod+click.

import type { IBufferCellPosition, ILink, ILinkProvider } from "@xterm/xterm";
import { type FileLink, findFileLinks } from "./file-links";

type CellLike = { getChars(): string; getWidth(): number };
type LineLike = { isWrapped: boolean; length: number; getCell(x: number): CellLike | undefined };
/** The part of xterm's active buffer this reads. */
export type BufferLike = { getLine(y: number): LineLike | undefined };

/** A wrapped line is one line of output; longer ones are cut, a path is rarely that long. */
const MAX_ROWS = 16;

/**
 * The text of the output line that buffer row `row` (from 0) is part of, with
 * wrapped rows joined, and for each UTF-16 unit the cell it is drawn in
 * (from 1, as xterm's link ranges count) and the cell where it ends.
 */
export function outputLine(
  buffer: BufferLike,
  row: number,
): { text: string; at: IBufferCellPosition[]; to: IBufferCellPosition[] } {
  let first = row;
  for (let up = 0; up < MAX_ROWS && first > 0 && buffer.getLine(first)?.isWrapped; up++) first--;
  let text = "";
  const at: IBufferCellPosition[] = [];
  const to: IBufferCellPosition[] = [];
  for (let y = first; y < first + MAX_ROWS; y++) {
    const line = buffer.getLine(y);
    if (!line || (y > first && !line.isWrapped)) break;
    for (let x = 0; x < line.length; x++) {
      const cell = line.getCell(x);
      const width = cell?.getWidth() ?? 1;
      // The second half of a wide character.
      if (width === 0) continue;
      const chars = cell?.getChars() || " ";
      text += chars;
      for (let i = 0; i < chars.length; i++) {
        at.push({ x: x + 1, y: y + 1 });
        to.push({ x: x + width, y: y + 1 });
      }
    }
  }
  return { text, at, to };
}

const sets = new WeakMap<readonly string[], ReadonlySet<string>>();

/** `files` as a set, made once per list (the list is cached until the files change). */
export function fileSet(files: readonly string[]): ReadonlySet<string> {
  let set = sets.get(files);
  if (!set) {
    set = new Set(files);
    sets.set(files, set);
  }
  return set;
}

export type FileLinkOptions = {
  buffer: () => BufferLike;
  /** The project's absolute folder. */
  root: () => string | null;
  /** The shell's starting folder, project-relative ("" for the root). */
  base: string;
  /** The project's files; a path is a link only when it is one of them. */
  files: () => Promise<ReadonlySet<string>>;
  /** Whether the click asks to open (Mod+click); a plain click selects text. */
  opens: (event: MouseEvent) => boolean;
  open: (link: FileLink) => void;
  /** Shows the hover hint over a link, or hides it with null. */
  hint: (text: string | null) => void;
  hintText: string;
};

export function fileLinkProvider(options: FileLinkOptions): ILinkProvider {
  return {
    provideLinks(row, callback) {
      const { text, at, to } = outputLine(options.buffer(), row - 1);
      const found = findFileLinks(text, options.root(), options.base).filter((link) => {
        const start = at[link.start];
        const end = to[link.end - 1];
        return start && end && start.y <= row && end.y >= row;
      });
      if (found.length === 0) return callback(undefined);
      options.files().then(
        (files) => {
          const links = found
            .filter((link) => files.has(link.path))
            .map(
              (link): ILink => ({
                // Checked in the filter above.
                range: { start: at[link.start] as IBufferCellPosition, end: to[link.end - 1] as IBufferCellPosition },
                text: text.slice(link.start, link.end),
                activate: (event) => {
                  if (options.opens(event)) options.open(link);
                },
                hover: () => options.hint(options.hintText),
                leave: () => options.hint(null),
              }),
            );
          callback(links.length > 0 ? links : undefined);
        },
        () => callback(undefined),
      );
    },
  };
}
