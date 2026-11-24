// Open files as editor buffers. A buffer keeps its CodeMirror state (text,
// undo history, selection) while its tab is in the background or the main
// area shows the viewer, so nothing typed is lost by switching. Saving uses
// `expected_mtime_ms`, outside changes to a buffer with unsaved edits become
// a conflict instead of replacing them, and unsaved text is mirrored to
// drafts so it survives a reload.

import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { highlightSelectionMatches, searchKeymap } from "@codemirror/search";
import { EditorSelection, EditorState, type Extension, type Text } from "@codemirror/state";
import { drawSelection, EditorView, highlightActiveLine, keymap, lineNumbers } from "@codemirror/view";
import { SvelteMap } from "svelte/reactivity";
import type { FileContent, ProjectId } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";
import type { Drafts } from "./drafts.svelte";
import type { HighlightClient, HighlightedDoc } from "./highlight/client";
import { treeSitterHighlighting } from "./highlight/extension";
import type { LanguageSource } from "./highlight/protocol";
import { type Language, languageFor, languageSource } from "./languages";
import { theme } from "./theme";

type BufferStatus = "loading" | "ready" | "binary" | "too_large" | "failed";

/** Unsaved text is written to drafts this long after the last keystroke. */
const DRAFT_DELAY_MS = 300;

type Options = {
  transport: Transport;
  project: ProjectId;
  drafts: Drafts;
  /** The highlight worker, or null to show plain text. */
  highlighter: () => HighlightClient | null;
};

export class Buffer {
  status = $state<BufferStatus>("loading");
  error = $state<string | null>(null);
  dirty = $state(false);
  /** The file changed on disk (or a save was refused) while there are unsaved edits. */
  conflict = $state(false);
  /** The file is gone from disk; saving recreates it. */
  deleted = $state(false);
  readonly language: Language | null;
  readonly loaded: Promise<void>;

  private state: EditorState | null = null;
  private view: EditorView | null = null;
  private saved: Text | null = null;
  private mtime = 0;
  private source: LanguageSource | null = null;
  private highlight: HighlightedDoc | null = null;
  private draftTimer: ReturnType<typeof setTimeout> | null = null;
  private disposed = false;

  constructor(
    private readonly options: Options,
    readonly path: string,
  ) {
    this.language = languageFor(path);
    this.loaded = this.load();
  }

  /** The current text, or null before the file is loaded. */
  get text(): string | null {
    return this.current?.doc.toString() ?? null;
  }

  private get current(): EditorState | null {
    return this.view?.state ?? this.state;
  }

  /** Shows this buffer in `view`, with its own history and selection. */
  attach(view: EditorView): void {
    if (!this.state) return;
    view.setState(this.state);
    this.view = view;
  }

  /** Takes the state back from the view, which is about to show something else. */
  detach(): void {
    if (this.view) this.state = this.view.state;
    this.view = null;
  }

  /** Writes the text; with `overwrite`, even if the file changed on disk. */
  async save(overwrite = false): Promise<boolean> {
    await this.loaded;
    const doc = this.current?.doc;
    if (!doc) return false;
    try {
      const { data } = await this.options.transport.call({
        method: "fs.write_file",
        params: {
          project: this.options.project,
          path: this.path,
          text: doc.toString(),
          expected_mtime_ms: overwrite ? null : this.mtime,
        },
      });
      this.mtime = data.mtime_ms;
      this.saved = doc;
      this.conflict = false;
      this.deleted = false;
      this.error = null;
      this.changed();
      return true;
    } catch (err) {
      if (err instanceof CoreError && err.code === "conflict") {
        this.conflict = true;
        this.deleted = err.message.includes("deleted on disk");
      } else {
        this.error = message(err);
      }
      return false;
    }
  }

  /** Drops unsaved edits and shows the file as it is on disk. */
  async revert(): Promise<void> {
    const file = await this.read();
    if (file?.body.kind === "text") this.replace(file.body.text, file.mtime_ms);
  }

  /** Called when the file may have changed on disk. */
  async diskChanged(): Promise<void> {
    if (this.status !== "ready") return;
    const file = await this.read(true);
    if (file === "not_found") {
      this.deleted = true;
      this.conflict = this.dirty;
      return;
    }
    this.deleted = false;
    if (file?.body.kind !== "text" || file.mtime_ms === this.mtime) return;
    if (this.dirty) this.conflict = true;
    else this.replace(file.body.text, file.mtime_ms);
  }

  /** Writes a pending draft now; false if unsaved text could not be stored. */
  flushDraft(): boolean {
    if (this.draftTimer) clearTimeout(this.draftTimer);
    this.draftTimer = null;
    const text = this.text;
    if (!this.dirty || text === null) return true;
    return this.options.drafts.set(this.options.project, this.path, { text, mtime: this.mtime });
  }

  /** Forgets the buffer and its draft. */
  dispose(): void {
    this.disposed = true;
    if (this.draftTimer) clearTimeout(this.draftTimer);
    this.options.drafts.remove(this.options.project, this.path);
    this.highlight?.close();
  }

  private async load(): Promise<void> {
    const file = await this.read();
    if (!file || this.disposed) return;
    if (file.body.kind !== "text") {
      this.status = file.body.kind;
      return;
    }
    const draft = this.options.drafts.get(this.options.project, this.path);
    try {
      this.source = this.language ? await languageSource(this.language) : null;
    } catch {
      this.source = null; // Unknown grammar or query: plain text.
    }
    if (this.disposed) return;
    this.mtime = file.mtime_ms;
    this.state = this.create(draft ? draft.text : file.body.text);
    this.saved = this.state.toText(file.body.text);
    // A draft made against another version of the file is a conflict, never a silent pick.
    this.conflict = draft !== null && draft.mtime !== file.mtime_ms;
    this.dirty = !this.state.doc.eq(this.saved);
    if (!this.dirty) this.options.drafts.remove(this.options.project, this.path);
    this.status = "ready";
  }

  /** Replaces the text with a fresh state, so undo cannot bring back text from before. */
  private replace(text: string, mtime: number): void {
    if (this.disposed) return;
    const previous = this.current;
    const state = this.create(text, previous?.selection.main.head);
    this.saved = state.doc;
    this.mtime = mtime;
    this.conflict = false;
    this.deleted = false;
    this.error = null;
    if (this.view) {
      const scroll = this.view.scrollDOM.scrollTop;
      this.view.setState(state);
      this.view.scrollDOM.scrollTop = scroll;
    } else {
      this.state = state;
    }
    this.changed();
  }

  /** Updates `dirty` and schedules or drops the draft. */
  private changed(): void {
    const doc = this.current?.doc;
    this.dirty = !!doc && !!this.saved && !doc.eq(this.saved);
    if (this.draftTimer) clearTimeout(this.draftTimer);
    this.draftTimer = null;
    if (this.dirty) this.draftTimer = setTimeout(() => this.flushDraft(), DRAFT_DELAY_MS);
    else this.options.drafts.remove(this.options.project, this.path);
  }

  private async read(): Promise<FileContent | null>;
  private async read(missingOk: true): Promise<FileContent | "not_found" | null>;
  private async read(missingOk = false): Promise<FileContent | "not_found" | null> {
    try {
      const { data } = await this.options.transport.call({
        method: "fs.read_file",
        params: { project: this.options.project, path: this.path },
      });
      return data;
    } catch (err) {
      if (missingOk && err instanceof CoreError && err.code === "not_found") return "not_found";
      if (!this.disposed) {
        this.error = message(err);
        if (this.status === "loading") this.status = "failed";
      }
      return null;
    }
  }

  private create(text: string, cursor = 0): EditorState {
    this.highlight?.close();
    const highlighter = this.source && this.options.highlighter();
    this.highlight = this.source && highlighter ? highlighter.open(this.source, text) : null;
    return EditorState.create({
      doc: text,
      selection: EditorSelection.cursor(Math.min(cursor, text.length)),
      extensions: this.extensions(),
    });
  }

  private extensions(): Extension[] {
    return [
      lineNumbers(),
      history(),
      drawSelection(),
      highlightActiveLine(),
      highlightSelectionMatches(),
      keymap.of([
        {
          key: "Mod-s",
          preventDefault: true,
          run: () => {
            void this.save();
            return true;
          },
        },
        indentWithTab,
        ...defaultKeymap,
        ...historyKeymap,
        ...searchKeymap,
      ]),
      EditorView.updateListener.of((update) => {
        if (update.docChanged) this.changed();
      }),
      theme,
      this.highlight ? treeSitterHighlighting(this.highlight) : [],
    ];
  }
}

/** The buffers of one project, by path. */
export class Buffers {
  private readonly buffers = new SvelteMap<string, Buffer>();
  private readonly unsubscribe: () => void;

  constructor(private readonly options: Options) {
    this.unsubscribe = options.transport.onEvent((event) => {
      if (event.type === "info.resync") {
        for (const buffer of this.buffers.values()) void buffer.diskChanged();
      } else if (event.type === "fs.changed" && event.data.project === options.project) {
        for (const path of event.data.paths) void this.buffers.get(path)?.diskChanged();
      }
    });
  }

  get project(): ProjectId {
    return this.options.project;
  }

  /** The buffer for `path`, loading it on first use. */
  open(path: string): Buffer {
    let buffer = this.buffers.get(path);
    if (!buffer) {
      buffer = new Buffer(this.options, path);
      this.buffers.set(path, buffer);
    }
    return buffer;
  }

  get(path: string): Buffer | undefined {
    return this.buffers.get(path);
  }

  /** Whether `path` has unsaved text, loaded or only as a draft. Reactive. */
  isDirty(path: string): boolean {
    const buffer = this.buffers.get(path);
    return buffer ? buffer.dirty : this.options.drafts.has(this.options.project, path);
  }

  /** Forgets `path`, including its unsaved text. */
  discard(path: string): void {
    const buffer = this.buffers.get(path);
    this.buffers.delete(path);
    if (buffer) buffer.dispose();
    else this.options.drafts.remove(this.options.project, path);
  }

  /** Writes every pending draft now; false if some unsaved text could not be stored. */
  flushDrafts(): boolean {
    let ok = true;
    for (const buffer of this.buffers.values()) ok = buffer.flushDraft() && ok;
    return ok;
  }

  /** Stops listening; drafts stay for the next time the project opens. */
  close(): void {
    this.flushDrafts();
    this.unsubscribe();
  }
}

function message(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}
