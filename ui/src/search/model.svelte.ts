// State of the search panel: the query, the running search and its results.
// Results stream in as `search.results` events; files are kept sorted by path.

import type { CoreEvent, FileMatches, ProjectId, SearchId, SearchQuery } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";

/** Quiet time after typing before a search starts. */
export const TYPING_DELAY_MS = 250;

export type SearchStatus =
  | { kind: "idle" }
  | { kind: "searching" }
  | { kind: "done"; truncated: boolean }
  | { kind: "error"; message: string };

/** Comma-separated globs as typed, as a list. */
export function splitGlobs(text: string): string[] {
  return text
    .split(",")
    .map((glob) => glob.trim())
    .filter((glob) => glob !== "");
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
  private readonly off: () => void;

  constructor(
    private readonly transport: Transport,
    private readonly project: ProjectId,
  ) {
    this.off = transport.onEvent((event) => this.onEvent(event));
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
      this.status = { kind: "error", message };
    }
  }

  /** Cancels the running search; results found so far stay. */
  stop(): void {
    if (this.current !== null && this.status.kind === "searching") {
      void this.cancel(this.current);
      this.status = { kind: "done", truncated: false };
    }
    this.current = null;
  }

  dispose(): void {
    if (this.timer !== null) clearTimeout(this.timer);
    this.stop();
    this.runs++;
    this.off();
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
      if (!event.data.cancelled) this.status = { kind: "done", truncated: event.data.truncated };
    }
  }
}
