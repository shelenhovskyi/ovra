// Open files as editor buffers. A buffer keeps its CodeMirror state (text,
// undo history, selection) while its tab is in the background or the main
// area shows the viewer, so nothing typed is lost by switching. Saving uses
// `expected_mtime_ms`, outside changes to a buffer with unsaved edits become
// a conflict instead of replacing them, and unsaved text is mirrored to
// drafts so it survives a reload. A buffer shown in both editor groups at
// once has a second, mirrored view: its own state with the same text, whose
// changes go to the buffer's state and back, and whose undo is the buffer's
// (CodeMirror's split-view pattern).

import {
  defaultKeymap,
  history,
  historyKeymap,
  indentLess,
  indentMore,
  insertNewlineAndIndent,
  redo,
  toggleBlockComment,
  undo,
} from "@codemirror/commands";
import { findPrevious, gotoLine, highlightSelectionMatches, searchKeymap } from "@codemirror/search";
import {
  Annotation,
  Compartment,
  EditorSelection,
  EditorState,
  type Extension,
  Prec,
  type Text,
  Transaction,
  type TransactionSpec,
} from "@codemirror/state";
import { drawSelection, EditorView, highlightActiveLine, type KeyBinding, keymap, lineNumbers } from "@codemirror/view";
import { SvelteMap } from "svelte/reactivity";
import { isApple, matchesShortcut, parseShortcut } from "../keys/shortcut";
import type { FileContent, ProjectId } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";
import type { AutoSaver } from "./autosave";
import type { Drafts } from "./drafts.svelte";
import { folding, foldKeys, isMarkdown } from "./folding";
import { gitExtension } from "./git/extension";
import { GitFile } from "./git/file";
import type { HighlightClient, HighlightedDoc } from "./highlight/client";
import { treeSitterHighlighting } from "./highlight/extension";
import type { LanguageSource } from "./highlight/protocol";
import { dedentLines, fileIndentation, indentLines, indentTab, newlineKeepIndent } from "./indent";
import { commentTokens, type Language, languageFor, languageSource } from "./languages";
import { editorPrefs } from "./prefs.svelte";
import { goToLine, runSearch, searchPhrases } from "./search";
import { detectIndent, lineEndingOf } from "./status.svelte";
import { theme } from "./theme";

/**
 * A binding for exactly `keys` (as in `Command.keys`), matched like the
 * global shortcuts: no extra modifier, physical key on non-Latin layouts.
 */
export function exactKey(keys: string, run: (view: EditorView) => boolean): KeyBinding {
  const shortcut = parseShortcut(keys, isApple());
  return { any: (view, event) => shortcut !== null && matchesShortcut(event, shortcut) && run(view) };
}

/**
 * The keys of every editor; `save` runs on Mod+S. CodeMirror's bindings that
 * would take a global shortcut keep only their Mac key: block comment
 * (Alt+Shift+A goes to approvals in a browser tab) and find previous on
 * Mod-Shift-g (Ctrl+Shift+G shows Git; Shift-F3 still finds previous).
 * Indenting and Enter follow the tab size preferences (`./indent`); go to
 * line names its field (`./search`).
 */
export function editorKeymap(save: () => void): KeyBinding[] {
  const macOnly = (binding: KeyBinding): KeyBinding => ({ ...binding, key: undefined, shift: undefined });
  const ownRun = new Map<KeyBinding["run"], KeyBinding["run"]>([
    [indentMore, indentLines],
    [indentLess, dedentLines],
    [insertNewlineAndIndent, newlineKeepIndent],
    [gotoLine, goToLine],
  ]);
  const ownKey = (binding: KeyBinding): KeyBinding[] => {
    if (binding.run === toggleBlockComment) return [macOnly(binding)];
    const run = binding.run && ownRun.get(binding.run);
    if (run) return [{ ...binding, run }];
    if (binding.key !== "Mod-g") return [binding];
    return [
      { ...binding, shift: undefined },
      { ...macOnly(binding), mac: "Mod-Shift-g", run: findPrevious },
    ];
  };
  return [
    // Exactly Mod+S: CodeMirror's `Mod-s` also matches with Shift held, and
    // would take Save all (Mod+Shift+S) from the global shortcuts.
    exactKey("Mod+S", () => {
      save();
      return true;
    }),
    // Replace's other key, where Mod+H is the global one (./search).
    exactKey("Mod+Alt+F", (view) => {
      runSearch("replace", view);
      return true;
    }),
    // Go to line on every platform, as in VS Code (CodeMirror's Mod-g finds the next match).
    { key: "Ctrl-g", run: goToLine, preventDefault: true },
    { key: "Tab", run: indentTab, shift: dedentLines },
    ...foldKeys,
    ...defaultKeymap.flatMap(ownKey),
    ...historyKeymap,
    ...searchKeymap.flatMap(ownKey),
  ];
}

/** Marks changes copied from the other view of a buffer, so they are not copied back. */
const mirrored = Annotation.define<boolean>();

type BufferStatus = "loading" | "ready" | "binary" | "too_large" | "failed";

/** Unsaved text is written to drafts this long after the last keystroke. */
const DRAFT_DELAY_MS = 300;

/**
 * Texts above either limit open without syntax highlighting or selection
 * match marks: both cost time on every edit and scroll, and minified files
 * gain nothing from them. First guesses; the speed gate tunes them.
 */
const HEAVY_TEXT_CHARS = 1024 * 1024;
const HEAVY_LINE_CHARS = 10_000;
/** Lines read to tell a file's indentation (detectIndent reads no more). */
const INDENT_SAMPLE = 2000;

/** Whether `text` is too big, or has too long a line, for the costly extensions. */
function isHeavy(text: string): boolean {
  if (text.length > HEAVY_TEXT_CHARS) return true;
  let start = 0;
  while (start < text.length) {
    const end = text.indexOf("\n", start);
    const stop = end < 0 ? text.length : end;
    if (stop - start > HEAVY_LINE_CHARS) return true;
    start = stop + 1;
  }
  return false;
}

type Options = {
  transport: Transport;
  project: ProjectId;
  drafts: Drafts;
  /** The highlight worker, or null to show plain text. */
  highlighter: () => HighlightClient | null;
  /** Told of every change, to save on its own as the preferences say. */
  autoSave?: AutoSaver;
};

export class Buffer {
  status = $state<BufferStatus>("loading");
  error = $state<string | null>(null);
  dirty = $state(false);
  /** The file changed on disk (or a save was refused) while there are unsaved edits. */
  conflict = $state(false);
  /** The text while it differs from the file on disk; reactive, for the viewer. */
  unsaved = $state<string | null>(null);
  /** The file is gone from disk; saving recreates it. */
  deleted = $state(false);
  /** The text is too large or has too long a line: no highlighting (see `isHeavy`). */
  heavy = $state(false);
  /** The file's line breaks; CodeMirror keeps "\n" inside, saving writes these. */
  lineEnding = $state<"LF" | "CRLF">("LF");
  /** Where the file is; a rename or move of it changes this (`moveTo`). */
  path = $state("");
  readonly language: Language | null;
  readonly loaded: Promise<void>;
  /** Git change markers and blame of this file. */
  readonly git: GitFile;

  private state: EditorState | null = null;
  private view: EditorView | null = null;
  /** A second view of the text while another editor group shows this buffer too. */
  private mirror: EditorView | null = null;
  private saved: Text | null = null;
  private mtime = 0;
  private source: LanguageSource | null = null;
  private highlight: HighlightedDoc | null = null;
  private draftTimer: ReturnType<typeof setTimeout> | null = null;
  private disposed = false;
  /** The language server's editor extension, if one serves this buffer. */
  private readonly server = new Compartment();
  private serverExtension: Extension = [];
  /** The text box's name, which says the path. */
  private readonly label = new Compartment();
  /** Line wrapping, tab size and indent style, as `editorPrefs` says. */
  private readonly prefs = new Compartment();
  /** The file's own indentation when it was read ("Tab", "Spaces: N"); it wins over the preferences. */
  private fileIndent: string | null = null;

  constructor(
    private readonly options: Options,
    path: string,
  ) {
    this.path = path;
    this.language = languageFor(path);
    this.git = new GitFile({ transport: options.transport, project: options.project, path });
    this.loaded = this.load();
  }

  /** The current document, or null before the file is loaded. */
  get doc(): Text | null {
    return this.current?.doc ?? null;
  }

  /** The current text, or null before the file is loaded. */
  get text(): string | null {
    return this.current?.doc.toString() ?? null;
  }

  private get current(): EditorState | null {
    return this.view?.state ?? this.state;
  }

  /**
   * Shows this buffer in `view`, with its own history and selection. While
   * another view shows it, `view` becomes a mirror of that one.
   */
  attach(view: EditorView): void {
    if (this.view && this.view !== view) {
      view.setState(this.mirrorState());
      this.mirror = view;
      return;
    }
    if (!this.state) return;
    view.setState(this.state);
    this.view = view;
  }

  /**
   * Takes the state back from `view` (or the buffer's own view), which is
   * about to show something else; a mirror left behind takes the state over.
   */
  detach(view: EditorView | null = this.view): void {
    if (view && view === this.mirror) {
      this.mirror = null;
      return;
    }
    if (!view || view !== this.view) return;
    this.state = view.state;
    this.view = null;
    const mirror = this.mirror;
    this.mirror = null;
    if (mirror) this.attach(mirror);
  }

  /**
   * Connects this buffer to a language server's extension (`[]` for none),
   * applying `also` (such as clearing diagnostics) in the same step.
   */
  setLanguageServer(extension: Extension, also?: (state: EditorState) => TransactionSpec): void {
    this.serverExtension = extension;
    const state = this.current;
    if (!state) return;
    const specs: TransactionSpec[] = [{ effects: this.server.reconfigure(extension) }];
    if (also) specs.push(also(state));
    if (this.view) this.view.dispatch(...specs);
    else this.state = state.update(...specs).state;
  }

  /** Follows the editor preferences (word wrap, tab size) as they are now. */
  applyPrefs(): void {
    const effects = this.prefs.reconfigure(prefsExtension(this.fileIndent));
    this.mirror?.dispatch({ effects });
    if (this.view) this.view.dispatch({ effects });
    else if (this.state) this.state = this.state.update({ effects }).state;
  }

  /**
   * The file was renamed or moved to `path`: text, undo history and unsaved
   * edits stay, the draft moves along. The language stays as it was.
   */
  moveTo(path: string): void {
    if (this.draftTimer) clearTimeout(this.draftTimer);
    this.draftTimer = null;
    this.options.drafts.remove(this.options.project, this.path);
    this.path = path;
    this.git.moveTo(path);
    this.flushDraft();
    const effects = this.label.reconfigure(this.labelExtension());
    this.mirror?.dispatch({ effects });
    if (this.view) this.view.dispatch({ effects });
    else if (this.state) this.state = this.state.update({ effects }).state;
  }

  /**
   * Changes the text as an edit the person can undo, shown or not; it stays
   * unsaved until saved. False if the buffer is not loaded.
   */
  edit(spec: TransactionSpec): boolean {
    const state = this.current;
    if (!state || this.status !== "ready") return false;
    if (this.view) {
      this.view.dispatch(spec);
    } else {
      this.state = state.update(spec).state;
      this.changed();
    }
    return true;
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
          text: doc.sliceString(0, doc.length, this.lineEnding === "CRLF" ? "\r\n" : "\n"),
          expected_mtime_ms: overwrite ? null : this.mtime,
        },
      });
      this.mtime = data.mtime_ms;
      this.saved = doc;
      this.conflict = false;
      this.deleted = false;
      this.error = null;
      this.changed();
      this.git.changed();
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
    if (file) this.error = null;
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
    this.options.autoSave?.cancel(this);
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
    this.lineEnding = lineEndingOf(file.body.text);
    this.state = this.create(draft ? draft.text : file.body.text);
    this.saved = this.state.toText(file.body.text);
    // A draft made against another version of the file is a conflict, never a silent pick.
    this.conflict = draft !== null && draft.mtime !== file.mtime_ms;
    this.dirty = !this.state.doc.eq(this.saved);
    this.unsaved = this.dirty ? this.state.doc.toString() : null;
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
    this.lineEnding = lineEndingOf(text);
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
    if (this.mirror) {
      const scroll = this.mirror.scrollDOM.scrollTop;
      this.mirror.setState(this.mirrorState());
      this.mirror.scrollDOM.scrollTop = scroll;
    }
    this.changed();
    this.git.changed();
  }

  /** Updates `dirty` and schedules or drops the draft. */
  private changed(): void {
    const doc = this.current?.doc;
    this.dirty = !!doc && !!this.saved && !doc.eq(this.saved);
    this.unsaved = this.dirty && doc ? doc.toString() : null;
    if (this.draftTimer) clearTimeout(this.draftTimer);
    this.draftTimer = null;
    this.options.autoSave?.edited(this);
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
    this.heavy = isHeavy(text);
    this.fileIndent = detectIndent(text.split("\n", INDENT_SAMPLE), "") || null;
    const highlighter = this.source && !this.heavy && this.options.highlighter();
    this.highlight = this.source && highlighter ? highlighter.open(this.source, text) : null;
    return EditorState.create({
      doc: text,
      selection: EditorSelection.cursor(Math.min(cursor, text.length)),
      extensions: this.extensions(),
    });
  }

  private labelExtension(): Extension {
    return EditorView.contentAttributes.of({ "aria-label": `Edit ${this.path}` });
  }

  /** A state for a mirror: the text and selection of the buffer's view, without history of its own. */
  private mirrorState(): EditorState {
    const state = this.current as EditorState;
    return EditorState.create({ doc: state.doc, selection: state.selection, extensions: this.extensions(true) });
  }

  /** Copies the changes of `update` to the other view showing this buffer, if there is one. */
  private copyChanges(from: EditorView, transactions: readonly Transaction[]): void {
    const to = from === this.view ? this.mirror : from === this.mirror ? this.view : null;
    if (!to) return;
    for (const tr of transactions) {
      if (tr.changes.empty || tr.annotation(mirrored)) continue;
      const userEvent = tr.annotation(Transaction.userEvent);
      const annotations = [mirrored.of(true), ...(userEvent ? [Transaction.userEvent.of(userEvent)] : [])];
      to.dispatch({ changes: tr.changes, annotations });
    }
  }

  /** The extensions of the buffer's own state, or with `mirror` of a mirror's, whose undo is the buffer's. */
  private extensions(mirror = false): Extension[] {
    const viaBuffer = (command: typeof undo) => () => !!this.view && command(this.view);
    return [
      lineNumbers(),
      folding(isMarkdown(this.path)),
      mirror
        ? Prec.high(
            keymap.of([
              { key: "Mod-z", run: viaBuffer(undo), preventDefault: true },
              { key: "Mod-y", mac: "Mod-Shift-z", run: viaBuffer(redo), preventDefault: true },
              { linux: "Ctrl-Shift-z", run: viaBuffer(redo), preventDefault: true },
            ]),
          )
        : history(),
      drawSelection(),
      highlightActiveLine(),
      this.heavy ? [] : highlightSelectionMatches(),
      keymap.of(editorKeymap(() => void this.save())),
      EditorView.updateListener.of((update) => {
        if (!update.docChanged) return;
        if (!mirror) this.changed();
        this.copyChanges(update.view, update.transactions);
      }),
      // Named, so a screen reader says which file the text box edits.
      this.label.of(this.labelExtension()),
      this.prefs.of(prefsExtension(this.fileIndent)),
      commentData(this.language),
      theme,
      searchPhrases,
      this.highlight ? treeSitterHighlighting(this.highlight, { edits: !mirror }) : [],
      // Language features live in the buffer's own view.
      mirror ? [] : this.server.of(this.serverExtension),
      gitExtension({ file: this.git, dirty: () => this.dirty, saved: () => this.saved }),
    ];
  }
}

/** The buffers of one project, by path. */
export class Buffers {
  private readonly buffers = new SvelteMap<string, Buffer>();
  private readonly unsubscribe: () => void;
  private readonly unsubscribeStatus: () => void;

  constructor(private readonly options: Options) {
    // After a reconnect, events from the outage are gone: check every buffer again.
    let wasOpen = true;
    this.unsubscribeStatus =
      options.transport.onStatus?.((status) => {
        if (status === "open" && !wasOpen) this.recheck();
        wasOpen = status === "open";
      }) ?? (() => {});
    this.unsubscribe = options.transport.onEvent((event) => {
      if (event.type === "info.resync") {
        this.recheck();
      } else if (event.type === "fs.changed" && event.data.project === options.project) {
        for (const path of event.data.paths) void this.buffers.get(path)?.diskChanged();
      } else if (event.type === "fs.git_changed" && event.data.project === options.project) {
        for (const buffer of this.buffers.values()) buffer.git.changed();
      }
    });
  }

  /** Rereads clean buffers and turns outside changes to dirty ones into conflicts. */
  private recheck(): void {
    for (const buffer of this.buffers.values()) void buffer.diskChanged();
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

  /** Unsaved text of `path`, loaded or only as a draft, or null. Reactive. */
  unsavedText(path: string): string | null {
    const buffer = this.buffers.get(path);
    if (buffer) return buffer.unsaved;
    return this.options.drafts.has(this.options.project, path)
      ? (this.options.drafts.get(this.options.project, path)?.text ?? null)
      : null;
  }

  /** Paths of loaded buffers with unsaved text. */
  unsavedPaths(): string[] {
    return [...this.buffers.values()].filter((b) => b.dirty).map((b) => b.path);
  }

  /**
   * The file at `from` is now at `to`: its buffer (or only its draft) moves
   * along; returns the moved buffer. A clean buffer whose language changes
   * with the name is dropped instead, so it opens fresh in the new one.
   */
  rename(from: string, to: string): Buffer | null {
    const buffer = this.buffers.get(from);
    const { drafts, project } = this.options;
    if (!buffer) {
      const draft = drafts.get(project, from);
      drafts.remove(project, from);
      if (draft) drafts.set(project, to, draft);
      return null;
    }
    this.buffers.delete(from);
    if (!buffer.dirty && languageFor(to)?.id !== buffer.language?.id) {
      buffer.dispose();
      return null;
    }
    buffer.moveTo(to);
    this.buffers.set(to, buffer);
    return buffer;
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
    this.unsubscribeStatus();
  }
}

/** The language's comment markers, so Mod+/ comments lines (there is no CodeMirror language here, only tree-sitter). */
function commentData(language: Language | null): Extension {
  const tokens = commentTokens(language);
  return tokens ? EditorState.languageData.of(() => [{ commentTokens: tokens }]) : [];
}

/** Word wrap, and the indentation of `fileIndent` (detected at load) or else the preferences. */
function prefsExtension(fileIndent: string | null): Extension {
  return [
    editorPrefs.wordWrap ? EditorView.lineWrapping : [],
    fileIndentation(fileIndent, editorPrefs.tabSize, editorPrefs.insertSpaces),
  ];
}

function message(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}
