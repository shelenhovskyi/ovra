// State of the search panel: the query, the running search and its results.
// Results stream in as `search.results` events; files are kept sorted by path.

import type { CoreEvent, FileMatches, ProjectId, SearchId, SearchQuery } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";

/** Quiet time after typing before a search starts. */
export const TYPING_DELAY_MS = 250;

export type SearchStatus =
  | { kind: "idle" }
  | { kind: "searching" }
  /** `stopped`: the person stopped it, so more may match. */
  | { kind: "done"; truncated: boolean; stopped: boolean }
  | { kind: "error"; message: string };

/** Comma-separated globs as typed, as a list. */
export function splitGlobs(text: string): string[] {
  return text
    .split(",")
    .map((glob) => glob.trim())
    .filter((glob) => glob !== "");
}

/** What the regex engine's error names, said in plain words. */
const REGEX_PROBLEMS: [string, string][] = [
  ["unclosed group", "a ( is not closed"],
  ["unopened group", "a ) has no ( before it"],
  ["unclosed character class", "a [ is not closed"],
  ["repetition operator missing expression", "*, + or ? has nothing before it to repeat"],
  ["unclosed counted repetition", "a { is not closed"],
  ["invalid escape", "a \\ is followed by a letter that means nothing here"],
  ["incomplete escape", "it ends with a \\"],
  ["invalid character class range", "a range in [ ] goes backwards"],
];

/**
 * The core's error in words for people: a pattern the regex engine cannot
 * read becomes one sentence that says how to search for the text as typed,
 * without the engine's own text (which shows a wrapper never typed).
 */
export function searchError(message: string): string {
  if (!message.startsWith("invalid search pattern:")) return message;
  const problem = REGEX_PROBLEMS.find(([engine]) => message.includes(engine))?.[1];
  const what = problem ? `Not a valid regular expression: ${problem}.` : "Not a valid regular expression.";
  return `${what} Turn off .* to search for the text as typed.`;
}

/** Inserts `more` into `files` (sorted by path) and returns the new list. */
export function mergeFiles(files: readonly FileMatches[], more: readonly FileMatches[]): FileMatches[] {
  const merged = [...files, ...more];
  merged.sort((a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0));
  return merged;
}

export class SearchModel {
  pattern = $state("");
  regex = $state(false);
  caseSensitive = $state(false);
  wholeWord = $state(false);
  include = $state("");
  exclude = $state("");

  /** Results so far; raw, since a search can hold thousands of lines. */
  files = $state.raw<FileMatches[]>([]);
  status = $state<SearchStatus>({ kind: "idle" });
  readonly lineCount = $derived(this.files.reduce((sum, file) => sum + file.lines.length, 0));

  /** The search whose results are shown. */
  private current: SearchId | null = null;
  /** Counts runs, so a reply to an older start is ignored. */
  private runs = 0;
  /** Events that arrived while a start was waiting for its id. */
  private early: CoreEvent[] | null = null;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private readonly offs: (() => void)[];

  constructor(
    private readonly transport: Transport,
    private readonly project: ProjectId,
  ) {
    // Missed events (a lagging connection or a reconnect) may include the
    // search's end: search again rather than wait forever.
    let wasOpen = true;
    this.offs = [
      transport.onEvent((event) => this.onEvent(event)),
      transport.onStatus?.((status) => {
        if (status === "open" && !wasOpen) this.retry();
        wasOpen = status === "open";
      }) ?? (() => {}),
    ];
  }

  /** Searches again once typing pauses. */
  schedule(): void {
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = setTimeout(() => {
      this.timer = null;
      void this.run();
    }, TYPING_DELAY_MS);
  }

  /** Stops the running search, if any, and searches for the current query now. */
  async run(): Promise<void> {
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = null;
    this.stop();
    const run = ++this.runs;
    this.files = [];
    if (this.pattern === "") {
      this.status = { kind: "idle" };
      return;
    }
    this.status = { kind: "searching" };
    this.early = [];
    try {
      const reply = await this.transport.call({
        method: "search.start",
        params: { project: this.project, query: this.query() },
      });
      if (run !== this.runs) {
        void this.cancel(reply.data.id);
        return;
      }
      this.current = reply.data.id;
      const early = this.early ?? [];
      this.early = null;
      for (const event of early) this.onEvent(event);
    } catch (error) {
      if (run !== this.runs) return;
      this.early = null;
      const message = error instanceof CoreError ? error.error.message : String(error);
      this.status = { kind: "error", message: searchError(message) };
    }
  }

  /** Cancels the running search; results found so far stay. */
  stop(): void {
    if (this.status.kind === "searching") {
      if (this.current !== null) void this.cancel(this.current);
      // A start still waiting for its id is cancelled when the id arrives.
      this.runs++;
      this.early = null;
      this.status = { kind: "done", truncated: false, stopped: true };
    }
    this.current = null;
  }

  dispose(): void {
    if (this.timer !== null) clearTimeout(this.timer);
    this.stop();
    this.runs++;
    for (const off of this.offs) off();
  }

  private retry(): void {
    if (this.status.kind === "searching") void this.run();
  }

  private query(): SearchQuery {
    return {
      pattern: this.pattern,
      regex: this.regex,
      case_sensitive: this.caseSensitive,
      whole_word: this.wholeWord,
      include: splitGlobs(this.include),
      exclude: splitGlobs(this.exclude),
    };
  }

  private async cancel(id: SearchId): Promise<void> {
    try {
      await this.transport.call({ method: "search.cancel", params: { id } });
    } catch {
      // The search ends by itself; nothing to show.
    }
  }

  private onEvent(event: CoreEvent): void {
    if (event.type === "info.resync") {
      this.retry();
      return;
    }
    if (event.type !== "search.results" && event.type !== "search.done") return;
    if (this.early) {
      this.early.push(event);
      return;
    }
    if (event.data.id !== this.current) return;
    if (event.type === "search.results") {
      this.files = mergeFiles(this.files, event.data.files);
    } else {
      this.current = null;
      if (!event.data.cancelled) this.status = { kind: "done", truncated: event.data.truncated, stopped: false };
    }
  }
}
