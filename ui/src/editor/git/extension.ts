// Git in the editor: change markers against the index in a gutter (unsaved
// edits included), a popup per change with "Stage hunk" and "Revert hunk",
// an optional blame gutter and a dimmed inline blame of the cursor line.
// Data comes from the buffer's `GitFile`; a buffer without git shows nothing.

import {
  type ChangeSpec,
  type Extension,
  Facet,
  RangeSet,
  StateEffect,
  StateField,
  type Text,
} from "@codemirror/state";
import {
  EditorView,
  GutterMarker,
  gutter,
  keymap,
  type Panel,
  showPanel,
  showTooltip,
  type Tooltip,
  ViewPlugin,
  type ViewUpdate,
} from "@codemirror/view";
import {
  blameExtension,
  blameGutterEffect,
  blameShown,
  inlineBlameEffect,
  toDisk,
  toggleBlameShown,
} from "./blame-view";
import type { GitFile } from "./file";
import {
  type ChangeKind,
  changeKind,
  diffLines,
  hunkAtLine,
  type LineHunk,
  MAX_DIFF_LINES,
  markerLine,
} from "./linediff";

/** What the extension needs from its buffer. */
export type GitHost = { file: GitFile; dirty: () => boolean; saved: () => Text | null };

/** Markers are recomputed this long after the last edit. */
const DIFF_DELAY_MS = 200;
/** The inline blame appears once the cursor rested this long. */
const INLINE_DELAY_MS = 500;

const host = Facet.define<GitHost, GitHost | null>({ combine: (values) => values[0] ?? null });

type Changes = { base: string[] | null; marks: RangeSet<GutterMarker>; tooLarge: boolean };
const NO_CHANGES: Changes = { base: null, marks: RangeSet.empty, tooLarge: false };
const setChanges = StateEffect.define<Changes>();

const changesField = StateField.define<Changes>({
  create: () => NO_CHANGES,
  update(value, tr) {
    for (const effect of tr.effects) if (effect.is(setChanges)) return effect.value;
    return tr.docChanged ? { ...value, marks: value.marks.map(tr.changes) } : value;
  },
  provide: (field) => showPanel.from(field, (value) => (value.tooLarge ? tooLargePanel : null)),
});

function tooLargePanel(): Panel {
  const dom = document.createElement("div");
  dom.className = "cm-git-note";
  dom.textContent = `Git change markers are off: this file has more than ${MAX_DIFF_LINES.toLocaleString("en")} lines.`;
  return { dom };
}

const DESCRIPTIONS: Record<ChangeKind, string> = {
  added: "Added lines",
  modified: "Changed lines",
  deleted: "Deleted lines",
};

class ChangeMarker extends GutterMarker {
  constructor(readonly kind: ChangeKind) {
    super();
  }
  override eq(other: ChangeMarker): boolean {
    return other.kind === this.kind;
  }
  override toDOM(): Node {
    const dom = document.createElement("span");
    dom.className = `cm-git-marker cm-git-${this.kind}`;
    dom.setAttribute("role", "img");
    dom.setAttribute("aria-label", `${DESCRIPTIONS[this.kind]}, click for actions`);
    dom.dataset.testid = "git-marker";
    dom.dataset.change = this.kind;
    return dom;
  }
}

const MARKERS: Record<ChangeKind, ChangeMarker> = {
  added: new ChangeMarker("added"),
  modified: new ChangeMarker("modified"),
  deleted: new ChangeMarker("deleted"),
};

/** The change markers of `hunks` in `doc`. */
function changeMarks(hunks: readonly LineHunk[], doc: Text): RangeSet<GutterMarker> {
  return RangeSet.of(hunks.map((h) => MARKERS[changeKind(h)].range(doc.line(markerLine(h) + 1).from)));
}

/** The edit that puts the index lines `old` back in place of `hunk`'s buffer lines. */
function revertChange(doc: Text, hunk: LineHunk, old: readonly string[]): ChangeSpec {
  const { newStart: start, newCount: count } = hunk;
  const text = old.join("\n");
  if (count > 0) {
    let from = doc.line(start + 1).from;
    let to = doc.line(start + count).to;
    // Removing whole lines takes a line break with them.
    if (old.length === 0 && start + count < doc.lines) to = doc.line(start + count + 1).from;
    else if (old.length === 0 && start > 0) from = doc.line(start).to;
    return { from, to, insert: text };
  }
  if (start < doc.lines) return { from: doc.line(start + 1).from, insert: `${text}\n` };
  return { from: doc.length, insert: `\n${text}` };
}

type Popup = { hunk: LineHunk; old: string[]; pos: number };
const setPopup = StateEffect.define<Popup | null>();

const popupField = StateField.define<Popup | null>({
  create: () => null,
  update(value, tr) {
    for (const effect of tr.effects) if (effect.is(setPopup)) return effect.value;
    return tr.docChanged ? null : value;
  },
  provide: (field) => showTooltip.from(field, (popup) => (popup ? popupTooltip(popup) : null)),
});

function popupTooltip(popup: Popup): Tooltip {
  return {
    pos: popup.pos,
    above: false,
    create(view) {
      const dom = popupDom(view, popup);
      return { dom, mount: () => dom.querySelector<HTMLButtonElement>("button:not([disabled])")?.focus() };
    },
  };
}

function closePopup(view: EditorView): void {
  view.dispatch({ effects: setPopup.of(null) });
  view.focus();
}

function popupDom(view: EditorView, popup: Popup): HTMLElement {
  const git = view.state.facet(host);
  const { hunk, old } = popup;
  const first = hunk.newStart + 1;
  const dom = element("div", "cm-git-popup");
  dom.setAttribute("role", "dialog");
  dom.setAttribute("aria-label", `Git change at line ${first}`);
  dom.dataset.testid = "git-change-popup";
  const caption = element("p", "cm-git-caption");
  caption.textContent = old.length > 0 ? "In the index:" : "New lines, not in the index.";
  const lines = element("pre", "cm-git-old");
  lines.textContent = old.join("\n");
  const status = element("p", "cm-git-status");
  status.setAttribute("role", "status");
  const actions = element("div", "cm-git-actions");
  const stage = button(actions, "Stage hunk");
  const revert = button(actions, "Revert hunk");
  button(actions, "Close").onclick = () => closePopup(view);
  if (!git || git.dirty()) {
    stage.disabled = true;
    status.textContent = "Save the file to stage this change.";
  }
  stage.onclick = async () => {
    if (!git) return;
    stage.disabled = true;
    status.textContent = "Staging...";
    const error = await git.file.stage(hunk);
    if (error === null) {
      closePopup(view);
    } else {
      status.textContent = error;
      stage.disabled = false;
    }
  };
  revert.onclick = () => {
    view.dispatch({ changes: revertChange(view.state.doc, hunk, old), userEvent: "input.revert" });
    closePopup(view);
  };
  dom.onkeydown = (event) => {
    if (event.code !== "Escape") return;
    event.preventDefault();
    event.stopPropagation();
    closePopup(view);
  };
  dom.append(caption, ...(old.length > 0 ? [lines] : []), actions, status);
  return dom;
}

function element(tag: string, className: string): HTMLElement {
  const dom = document.createElement(tag);
  dom.className = className;
  return dom;
}

function button(parent: HTMLElement, label: string): HTMLButtonElement {
  const dom = document.createElement("button");
  dom.type = "button";
  dom.textContent = label;
  parent.append(dom);
  return dom;
}

/** Opens the popup for the change under buffer line `line` (0-based); false if there is none. */
function openChange(view: EditorView, line: number): boolean {
  const { base } = view.state.field(changesField);
  if (!base) return false;
  // Fresh, so a pending debounce cannot offer a stale hunk.
  const hunk = hunkAtLine(diffLines(base, view.state.doc.toJSON()), line);
  if (!hunk) return false;
  const pos = view.state.doc.line(markerLine(hunk) + 1).from;
  const old = base.slice(hunk.oldStart, hunk.oldStart + hunk.oldCount);
  view.dispatch({ effects: setPopup.of({ hunk, old, pos }) });
  return true;
}

/** The controller of the editor view, for the commands. */
let current: Controller | null = null;

class Controller {
  private diffTimer: ReturnType<typeof setTimeout> | undefined;
  private inlineTimer: ReturnType<typeof setTimeout> | undefined;
  private readonly unsubscribe: () => void;
  private destroyed = false;

  constructor(readonly view: EditorView) {
    current = this;
    this.unsubscribe =
      this.git?.file.subscribe(() => {
        this.refresh(0);
        this.scheduleInline();
      }) ?? (() => {});
    this.refresh(0);
    this.scheduleInline();
  }

  private get git(): GitHost | null {
    return this.view.state.facet(host);
  }

  update(update: ViewUpdate): void {
    if (update.docChanged) this.refresh(DIFF_DELAY_MS);
    if (update.docChanged || update.selectionSet) this.scheduleInline();
  }

  destroy(): void {
    this.destroyed = true;
    clearTimeout(this.diffTimer);
    clearTimeout(this.inlineTimer);
    this.unsubscribe();
    if (current === this) current = null;
  }

  /** Recomputes markers (and the blame gutter) after `delay` ms. */
  refresh(delay: number): void {
    clearTimeout(this.diffTimer);
    this.diffTimer = setTimeout(() => void this.recompute(), delay);
  }

  /** Opens the popup for the change at the cursor line. */
  openAtCursor(): void {
    const { state } = this.view;
    openChange(this.view, state.doc.lineAt(state.selection.main.head).number - 1);
  }

  private async recompute(): Promise<void> {
    const git = this.git;
    const base = git ? await git.file.indexLines() : null;
    if (this.destroyed) return;
    const doc = this.view.state.doc;
    let changes = NO_CHANGES;
    if (base && (base.length > MAX_DIFF_LINES || doc.lines > MAX_DIFF_LINES)) {
      changes = { ...NO_CHANGES, tooLarge: true };
    } else if (base) {
      changes = { base, marks: changeMarks(diffLines(base, doc.toJSON()), doc), tooLarge: false };
    }
    this.view.dispatch({ effects: setChanges.of(changes) });
    await this.renderBlameGutter();
  }

  /** Fills or empties the blame gutter. */
  async renderBlameGutter(): Promise<void> {
    const git = this.git;
    const blame = git && blameShown() ? await git.file.blame() : null;
    if (this.destroyed) return;
    const { state } = this.view;
    const effect = blameGutterEffect(state, blame, git && toDisk(state, git.saved(), git.dirty()));
    if (effect) this.view.dispatch({ effects: effect });
  }

  private scheduleInline(): void {
    clearTimeout(this.inlineTimer);
    this.inlineTimer = setTimeout(() => void this.showInline(), INLINE_DELAY_MS);
  }

  private async showInline(): Promise<void> {
    const git = this.git;
    const blame = git ? await git.file.blame() : null;
    if (!git || !blame || this.destroyed) return;
    const { state } = this.view;
    const map = toDisk(state, git.saved(), git.dirty());
    if (map) this.view.dispatch({ effects: inlineBlameEffect(state, blame, map) });
  }
}

const controller = ViewPlugin.fromClass(Controller);

/** Shows or hides the blame gutter. */
export function toggleBlame(): void {
  toggleBlameShown();
  void current?.renderBlameGutter();
}

/** Opens the change popup for the cursor line of the editor, if it is on a change. */
export function showChangeAtCursor(): void {
  current?.openAtCursor();
}

const styles = EditorView.baseTheme({
  ".cm-git-gutter .cm-gutterElement": { width: "4px", padding: "0 2px", cursor: "pointer" },
  ".cm-git-marker": { display: "block", height: "100%" },
  ".cm-git-added": { backgroundColor: "#4caf50" },
  ".cm-git-modified": { backgroundColor: "#4f8fd8" },
  ".cm-git-deleted": { borderBottom: "3px solid #e05252", height: "auto", marginTop: "calc(1.5em - 3px)" },
  ".cm-git-popup": { padding: "6px 8px", maxWidth: "60ch", backgroundColor: "#1d1d23", color: "#e6e6e6" },
  ".cm-git-old": { margin: "4px 0", maxHeight: "12em", overflow: "auto", color: "#e8a0a0" },
  ".cm-git-caption, .cm-git-status": { margin: "0" },
  ".cm-git-actions": { display: "flex", gap: "6px", margin: "4px 0" },
  ".cm-git-note": { padding: "2px 8px", color: "#9a9aa6" },
});

/** The git extension of one buffer. */
export function gitExtension(git: GitHost): Extension {
  return [
    host.of(git),
    changesField,
    popupField,
    blameExtension,
    controller,
    gutter({
      class: "cm-git-gutter",
      markers: (view) => view.state.field(changesField).marks,
      domEventHandlers: {
        mousedown: (view, line) => openChange(view, view.state.doc.lineAt(line.from).number - 1),
      },
    }),
    keymap.of([
      {
        key: "Escape",
        run: (view) => {
          if (!view.state.field(popupField)) return false;
          closePopup(view);
          return true;
        },
      },
    ]),
    styles,
  ];
}
