// One file open in the editor: loads it, tracks unsaved changes and saves
// with `expected_mtime_ms` so a file changed on disk is never overwritten
// silently. Free of Svelte components so tests can drive it directly.

import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { highlightSelectionMatches, searchKeymap } from "@codemirror/search";
import { EditorState, type Extension, type Text } from "@codemirror/state";
import { drawSelection, EditorView, highlightActiveLine, keymap, lineNumbers } from "@codemirror/view";
import { SvelteSet } from "svelte/reactivity";
import type { FileContent, ProjectId } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";
import type { HighlightClient } from "./highlight/client";
import { treeSitterHighlighting } from "./highlight/extension";
import type { LanguageSource } from "./highlight/protocol";
import { type Language, languageFor, languageSource } from "./languages";
import { theme } from "./theme";

type SessionStatus = "loading" | "ready" | "binary" | "too_large" | "failed";

type Options = {
  transport: Transport;
  project: ProjectId;
  path: string;
  parent: HTMLElement;
  /** The highlight worker, or null to show plain text. */
  highlighter: () => HighlightClient | null;
};

/** Unsaved text of files the user switched away from, by project and path. */
const unsaved = new Map<string, { text: string; saved: string; mtime: number }>();

/** Files with unsaved changes, open or stashed, by `fileKey`. */
const dirtyFiles = new SvelteSet<string>();

function fileKey(project: ProjectId, path: string): string {
  return `${project}\u0000${path}`;
}

/** Whether a file has unsaved changes in the editor. Reactive. */
export function hasUnsavedChanges(project: ProjectId, path: string): boolean {
  return dirtyFiles.has(fileKey(project, path));
}

export class EditorSession {
  status = $state<SessionStatus>("loading");
  error = $state<string | null>(null);
  #dirty = $state(false);
  /** The file changed on disk since it was loaded; saving needs a choice. */
  conflict = $state(false);
  readonly language: Language | null;

  private view: EditorView | null = null;
  private saved: Text | null = null;
  private mtime = 0;
  private destroyed = false;
  private readonly unsubscribe: () => void;

  constructor(private readonly options: Options) {
    this.language = languageFor(options.path);
    this.unsubscribe = options.transport.onEvent((event) => {
      if (this.dirty) return;
      if (event.type === "info.resync") void this.refresh();
      if (event.type !== "fs.changed" || event.data.project !== options.project) return;
      if (event.data.paths.includes(options.path)) void this.refresh();
    });
  }

  private get key(): string {
    return fileKey(this.options.project, this.options.path);
  }

  get dirty(): boolean {
    return this.#dirty;
  }

  private set dirty(value: boolean) {
    this.#dirty = value;
    if (value) dirtyFiles.add(this.key);
    else dirtyFiles.delete(this.key);
  }

  /** The editor text, or null before the file is loaded. */
  get text(): string | null {
    return this.view?.state.doc.toString() ?? null;
  }

  async load(): Promise<void> {
    const file = await this.read();
    if (!file || this.destroyed) return;
    if (file.body.kind !== "text") {
      this.status = file.body.kind;
      return;
    }
    const stash = unsaved.get(this.key);
    unsaved.delete(this.key);
    // Unsaved text survives switching files, unless the file changed meanwhile.
    const keep = stash?.mtime === file.mtime_ms ? stash : null;
    const text = keep ? keep.text : file.body.text;
    this.mtime = file.mtime_ms;
    const source = await this.languageSource();
    if (this.destroyed) return;
    const state = EditorState.create({ doc: text, extensions: this.extensions(text, source) });
    this.saved = keep ? state.toText(keep.saved) : state.doc;
    this.dirty = !state.doc.eq(this.saved);
    this.view = new EditorView({ parent: this.options.parent, state });
    this.status = "ready";
  }

  /** Writes the text; with `overwrite`, even if the file changed on disk. */
  async save(overwrite = false): Promise<void> {
    const view = this.view;
    if (!view) return;
    const doc = view.state.doc;
    try {
      const { data } = await this.options.transport.call({
        method: "fs.write_file",
        params: {
          project: this.options.project,
          path: this.options.path,
          text: doc.toString(),
          expected_mtime_ms: overwrite ? null : this.mtime,
        },
      });
      this.mtime = data.mtime_ms;
      this.saved = doc;
      this.dirty = !view.state.doc.eq(doc);
      this.conflict = false;
      this.error = null;
    } catch (err) {
      if (err instanceof CoreError && err.code === "conflict") this.conflict = true;
      else this.error = message(err);
    }
  }

  /** Drops local changes and shows the file as it is on disk. */
  async reload(): Promise<void> {
    const file = await this.read();
    if (file?.body.kind === "text") this.replace(file.body.text, file.mtime_ms);
  }

  destroy(): void {
    this.destroyed = true;
    this.unsubscribe();
    if (this.view && this.dirty && this.saved) {
      unsaved.set(this.key, { text: this.view.state.doc.toString(), saved: this.saved.toString(), mtime: this.mtime });
    } else if (this.dirty) {
      this.dirty = false;
    }
    this.view?.destroy();
    this.view = null;
  }

  private async refresh(): Promise<void> {
    const file = await this.read();
    if (file?.body.kind === "text" && file.mtime_ms !== this.mtime && !this.dirty) {
      this.replace(file.body.text, file.mtime_ms);
    }
  }

  private replace(text: string, mtime: number): void {
    const view = this.view;
    if (!view || this.destroyed) return;
    view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: text } });
    this.saved = view.state.doc;
    this.mtime = mtime;
    this.dirty = false;
    this.conflict = false;
  }

  private async read(): Promise<FileContent | null> {
    try {
      const { data } = await this.options.transport.call({
        method: "fs.read_file",
        params: { project: this.options.project, path: this.options.path },
      });
      return data;
    } catch (err) {
      if (!this.destroyed) {
        this.error = message(err);
        if (this.status === "loading") this.status = "failed";
      }
      return null;
    }
  }

  private async languageSource(): Promise<LanguageSource | null> {
    try {
      return this.language ? await languageSource(this.language) : null;
    } catch {
      return null; // Unknown grammar or query: plain text.
    }
  }

  private extensions(text: string, source: LanguageSource | null): Extension[] {
    const highlighter = source && this.options.highlighter();
    const highlighting = source && highlighter ? treeSitterHighlighting(highlighter.open(source, text)) : [];
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
        if (update.docChanged) this.dirty = !update.state.doc.eq(this.saved ?? update.state.doc);
      }),
      theme,
      highlighting,
    ];
  }
}

function message(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}
