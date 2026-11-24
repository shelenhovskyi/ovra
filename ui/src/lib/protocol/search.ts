import type { ProjectId } from "./projects";

export type SearchId = number;

export type SearchQuery = {
  /** Text to find on one line; a regular expression (Rust/ripgrep syntax) with `regex`. Not empty. */
  pattern: string;
  regex?: boolean;
  case_sensitive?: boolean;
  /** Only matches that start and end at a word boundary. */
  whole_word?: boolean;
  /**
   * Globs over project-relative paths; when not empty, only matching files are
   * searched. A glob without `/` matches a file or folder name at any depth
   * (`*.rs`, `src`), otherwise from the root. A folder matches what is inside it.
   */
  include?: string[];
  /** Like `include`; matching files are skipped. */
  exclude?: string[];
};

export type SearchRequest =
  /**
   * Searches the files `fs.list_files` lists, skipping binaries, symlinks and
   * files over 16 MiB. At most four run at once; another cancels the oldest.
   * Answers at once; results follow as events. `bad_request` for an empty
   * pattern or an invalid regex or glob.
   */
  | { method: "search.start"; params: { project: ProjectId; query: SearchQuery } }
  /** A `search.done` with `cancelled` follows unless the search already ended. */
  | { method: "search.cancel"; params: { id: SearchId } };

export type SearchResponse = { type: "search.started"; data: { id: SearchId } };

export type LineMatch = {
  /** From 1. */
  line: number;
  /** The line without its break; a long line is cut to a window around its first match. */
  text: string;
  /** UTF-16 offset in the full line where `text` starts (0 unless cut). */
  column: number;
  /** `[start, end)` UTF-16 offsets into `text`, in order. */
  ranges: [number, number][];
};

/** One file's matching lines, in line order; a file appears in one event of a search at most. */
export type FileMatches = { path: string; lines: LineMatch[] };

export type SearchEvent =
  | { type: "search.results"; data: { id: SearchId; files: FileMatches[] } }
  /** `truncated`: stopped at 10,000 lines (MAX_SEARCH_LINES in Rust) and left matches or files out. */
  | {
      type: "search.done";
      data: { id: SearchId; files: number; lines: number; truncated: boolean; cancelled: boolean };
    };
