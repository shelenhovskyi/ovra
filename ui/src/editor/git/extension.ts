// Git in the editor: change markers against the index in a gutter (unsaved
// edits included), a popup per change with "Stage hunk" and "Revert hunk",
// an optional blame gutter and a dimmed inline blame of the cursor line.
// The gutters are hidden from screen readers, so the commands (next and
// previous change, change at cursor, blame) announce what they find.
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
  cursorBlame,
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

/** What a change is and where, for people: "Changed lines 4 to 6", "Deleted line after line 19". */
export function describeChange(hunk: LineHunk): string {
  const kind = changeKind(hunk);
  if (kind === "deleted") {
    const what = hunk.oldCount === 1 ? "Deleted line" : `Deleted ${hunk.oldCount} lines`;
    return hunk.newStart === 0 ? `${what} at the start` : `${what} after line ${hunk.newStart}`;
  }
  const first = hunk.newStart + 1;
  const name = kind === "added" ? "Added" : "Changed";
  return hunk.newCount === 1 ? `${name} line ${first}` : `${name} lines ${first} to ${first + hunk.newCount - 1}`;
}

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
    // The gutters are aria-hidden: the title is for the mouse, the commands announce changes.
    dom.title = `${DESCRIPTIONS[this.kind]}, click for actions`;
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

/** The change markers of `hunks` in `doc`: one on every added or changed line, one per deletion. */
function changeMarks(hunks: readonly LineHunk[], doc: Text): RangeSet<GutterMarker> {
  return RangeSet.of(
    hunks.flatMap((h) => {
      const marker = MARKERS[changeKind(h)];
      const lines = Array.from({ length: Math.max(h.newCount, 1) }, (_, i) => markerLine(h) + i);
      return lines.map((line) => marker.range(doc.line(line + 1).from));
    }),
  );
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
      // The first safe action: Stage, or Close when Stage is off. Never Revert.
      const first = dom.querySelector<HTMLButtonElement>("button:not([disabled]):not([data-destructive])");
      return { dom, mount: () => first?.focus() };
    },
  };
}

/** Closes the popup; focus goes back to the text unless it already moved elsewhere (`refocus` false). */
function closePopup(view: EditorView, refocus = true): void {
  if (view.state.field(popupField)) view.dispatch({ effects: setPopup.of(null) });
  if (refocus) view.focus();
}

function popupDom(view: EditorView, popup: Popup): HTMLElement {
  const git = view.state.facet(host);
  const { hunk, old } = popup;
  const dom = element("div", "cm-git-popup");
  dom.setAttribute("role", "dialog");
  dom.setAttribute("aria-modal", "true");
  dom.setAttribute("aria-label", describeChange(hunk));
  dom.dataset.testid = "git-change-popup";
  const caption = element("p", "cm-git-caption");
  caption.textContent = old.length > 0 ? "Before this change:" : "New lines: nothing was here before.";
  const lines = element("pre", "cm-git-old");
  lines.textContent = old.join("\n");
  const status = element("p", "cm-git-status");
  status.setAttribute("role", "status");
  const actions = element("div", "cm-git-actions");
  const stage = button(actions, "Stage hunk");
  const revert = button(actions, "Revert hunk");
  revert.dataset.destructive = "";
  button(actions, "Close").onclick = () => closePopup(view);
  if (!git || git.dirty()) {
    stage.disabled = true;
    status.textContent = "Save the file to stage this change.";
  }
  let staging = false;
  stage.onclick = async () => {
    if (!git || staging) return;
    // aria-disabled, not disabled: disabling the focused button would drop focus out of the popup.
    staging = true;
    stage.setAttribute("aria-disabled", "true");
    status.textContent = "Staging...";
    const error = await git.file.stage(hunk);
    staging = false;
    stage.removeAttribute("aria-disabled");
    if (error === null) {
      closePopup(view);
      announce(view, "Hunk staged.");
    } else status.textContent = error;
  };
  revert.onclick = () => {
    // The edit closes the popup; the revert is only in the buffer until it is saved.
    view.dispatch({
      changes: revertChange(view.state.doc, hunk, old),
      userEvent: "input.revert",
      effects: EditorView.announce.of("Change reverted; save to keep it."),
    });
    closePopup(view);
  };
  dom.onkeydown = (event) => {
    if (event.code === "Tab") trapTab(dom, event);
    if (event.code !== "Escape") return;
    event.preventDefault();
    event.stopPropagation();
    closePopup(view);
  };
  // Focus that moves elsewhere (a click in the text or another panel) closes it.
  dom.addEventListener("focusout", (event) => {
    const next = event.relatedTarget;
    if (next instanceof Node && !dom.contains(next)) closePopup(view, false);
  });
  dom.append(caption, ...(old.length > 0 ? [lines] : []), actions, status);
  return dom;
}

/** Keeps Tab and Shift+Tab cycling through the popup's buttons. */
function trapTab(dom: HTMLElement, event: KeyboardEvent): void {
  const buttons = [...dom.querySelectorAll<HTMLButtonElement>("button:not([disabled])")];
  const at = buttons.indexOf(document.activeElement as HTMLButtonElement);
  event.preventDefault();
  buttons[(at + (event.shiftKey ? -1 : 1) + buttons.length) % buttons.length]?.focus();
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

/** The buffer's changes against the index, fresh, so a pending debounce cannot offer a stale hunk. */
function currentHunks(view: EditorView): { base: string[]; hunks: LineHunk[] } | null {
  const { base } = view.state.field(changesField);
  return base ? { base, hunks: diffLines(base, view.state.doc.toJSON()) } : null;
}

/**
 * Opens the popup for the change under buffer line `line` (0-based); false if
 * there is none. An open popup is replaced, or closed when there is no change.
 */
function openChange(view: EditorView, line: number): boolean {
  const found = currentHunks(view);
  const hunk = found && hunkAtLine(found.hunks, line);
  if (!found || !hunk) {
    if (view.state.field(popupField)) view.dispatch({ effects: setPopup.of(null) });
    return false;
  }
  const pos = view.state.doc.line(markerLine(hunk) + 1).from;
  const old = found.base.slice(hunk.oldStart, hunk.oldStart + hunk.oldCount);
  view.dispatch({ effects: setPopup.of({ hunk, old, pos }) });
  return true;
}

/** Says `message` through the editor's live region. */
function announce(view: EditorView, message: string): void {
  view.dispatch({ effects: EditorView.announce.of(message) });
}

/** The controller of the editor view, for the commands. */
let current: Controller | null = null;
/** Waiting for an editor view's first markers, see `editorReady`. */
let waiting: (() => void)[] = [];

/** Resolves once the next editor view knows its git changes, so a command can act on them. */
export function editorReady(): Promise<void> {
  return new Promise((resolve) => waiting.push(resolve));
}

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

  private cursorLine(): number {
    const { state } = this.view;
    return state.doc.lineAt(state.selection.main.head).number - 1;
  }

  /** Opens the popup for the change at the cursor line, or says there is none. */
  openAtCursor(): void {
    if (!openChange(this.view, this.cursorLine())) announce(this.view, "No git change on this line.");
  }

  /** Moves the cursor to the next (`step` 1) or previous (-1) change, wrapping around, and announces it. */
  goToChange(step: 1 | -1): void {
    const hunks = currentHunks(this.view)?.hunks ?? [];
    if (hunks.length === 0) {
      announce(this.view, "No git changes in this file.");
      return;
    }
    const line = this.cursorLine();
    let index = step > 0 ? 0 : hunks.length - 1;
    if (step > 0) {
      const ahead = hunks.findIndex((h) => markerLine(h) > line);
      if (ahead >= 0) index = ahead;
    } else {
      // Back from inside a change goes to the one before it, not to its own start.
      for (const [i, h] of hunks.entries()) if (markerLine(h) < line && !hunkAtLine([h], line)) index = i;
    }
    const hunk = hunks[index] as LineHunk;
    const pos = this.view.state.doc.line(markerLine(hunk) + 1).from;
    this.view.dispatch({
      selection: { anchor: pos },
      effects: [
        EditorView.scrollIntoView(pos, { y: "center" }),
        EditorView.announce.of(`${describeChange(hunk)}, ${index + 1} of ${hunks.length}`),
      ],
    });
    this.view.focus();
  }

  /** Announces the blame of the cursor line. */
  async announceBlame(): Promise<void> {
    const git = this.git;
    const blame = git ? await git.file.blame() : null;
    if (this.destroyed) return;
    const { state } = this.view;
    const map = git && toDisk(state, git.saved(), git.dirty());
    announce(this.view, (blame && map && cursorBlame(state, blame, map)) || "No blame for this line.");
  }

  announce(message: string): void {
    announce(this.view, message);
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
    if (current === this) {
      const ready = waiting;
      waiting = [];
      for (const resolve of ready) resolve();
    }
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
    const effect = map && inlineBlameEffect(state, blame, map);
    if (effect) this.view.dispatch({ effects: effect });
  }
}

const controller = ViewPlugin.fromClass(Controller);

/** Shows or hides the blame gutter, and says which. */
export function toggleBlame(): void {
  toggleBlameShown();
  current?.announce(blameShown() ? "Blame shown." : "Blame hidden.");
  void current?.renderBlameGutter();
}

/** Opens the change popup for the cursor line of the editor, or says there is no change. */
export function showChangeAtCursor(): void {
  current?.openAtCursor();
}

/** Moves the editor's cursor to its next (`step` 1) or previous (-1) git change. */
export function goToChange(step: 1 | -1): void {
  current?.goToChange(step);
}

/** Announces the blame of the editor's cursor line. */
export async function readBlame(): Promise<void> {
  await current?.announceBlame();
}

const styles = EditorView.baseTheme({
  // The cell is border-box: 8px less 2px of padding a side leaves the marker 4px.
  ".cm-git-gutter .cm-gutterElement": { width: "8px", padding: "0 2px", cursor: "pointer" },
  // Kinds differ by shape, not only by colour: a solid bar, a striped bar, a triangle.
  ".cm-git-marker": { display: "block", boxSizing: "border-box", width: "4px", height: "100%" },
  ".cm-git-added": { borderLeft: "4px solid var(--ok)" },
  ".cm-git-modified": { background: "repeating-linear-gradient(var(--accent) 0 3px, transparent 3px 5px)" },
  ".cm-git-deleted": {
    width: "0",
    height: "0",
    marginTop: "calc(1.5em - 5px)",
    borderLeft: "6px solid var(--danger)",
    borderTop: "5px solid transparent",
    borderBottom: "5px solid transparent",
  },
  "@media (forced-colors: active)": {
    ".cm-git-marker": { forcedColorAdjust: "none" },
    ".cm-git-added": { borderLeftColor: "CanvasText" },
    ".cm-git-modified": { background: "repeating-linear-gradient(Highlight 0 3px, transparent 3px 5px)" },
    ".cm-git-deleted": { borderLeftColor: "LinkText" },
  },
  ".cm-git-popup": { padding: "8px 10px", maxWidth: "60ch", backgroundColor: "var(--s4)", color: "var(--text)" },
  ".cm-git-old": { margin: "4px 0", maxHeight: "12em", overflow: "auto", color: "var(--danger)" },
  ".cm-git-caption, .cm-git-status": { margin: "0" },
  ".cm-git-actions": { display: "flex", gap: "6px", margin: "4px 0" },
  ".cm-git-note": { padding: "2px 8px", color: "var(--text-2)" },
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
