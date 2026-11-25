// The editor side of blame: an optional gutter with the author and age of
// each block of lines, and a dimmed inline blame after the cursor line.
// The git extension's controller decides when; this builds what is shown.

import { type EditorState, Prec, RangeSet, StateEffect, StateField, type Text } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, GutterMarker, gutter, WidgetType } from "@codemirror/view";
import type { Blame } from "../../lib/protocol";
import { blameBlocks, blockLabel, commitsByLine, lineBlame } from "./blame";
import { diffLines, MAX_DIFF_LINES, mapLine } from "./linediff";

/** Maps a buffer line to its line on disk, or null for an edited line. */
type ToDisk = (line: number) => number | null;

class BlameMarker extends GutterMarker {
  constructor(readonly label: string) {
    super();
  }
  override eq(other: BlameMarker): boolean {
    return other.label === this.label;
  }
  override toDOM(): Node {
    const dom = document.createElement("span");
    dom.className = "cm-git-blame";
    dom.dataset.testid = "git-blame";
    dom.textContent = this.label;
    return dom;
  }
}

class InlineBlame extends WidgetType {
  constructor(readonly text: string) {
    super();
  }
  override eq(other: InlineBlame): boolean {
    return other.text === this.text;
  }
  toDOM(): HTMLElement {
    const dom = document.createElement("span");
    dom.className = "cm-git-inline-blame";
    // Not part of the line's text for a screen reader; "Git: read blame of cursor line" says it.
    dom.setAttribute("aria-hidden", "true");
    dom.dataset.testid = "git-inline-blame";
    dom.textContent = this.text;
    return dom;
  }
}

type BlameView = { gutter: RangeSet<GutterMarker>; inline: DecorationSet };
const setBlameGutter = StateEffect.define<RangeSet<GutterMarker>>();
const setInlineBlame = StateEffect.define<DecorationSet>();

const blameField = StateField.define<BlameView>({
  create: () => ({ gutter: RangeSet.empty, inline: Decoration.none }),
  update(value, tr) {
    let next = value;
    // The inline blame is of the cursor line: it goes when the cursor moves.
    if (tr.docChanged) next = { gutter: value.gutter.map(tr.changes), inline: Decoration.none };
    else if (tr.selection) next = { ...value, inline: Decoration.none };
    for (const effect of tr.effects) {
      if (effect.is(setBlameGutter)) next = { ...next, gutter: effect.value };
      if (effect.is(setInlineBlame)) next = { ...next, inline: effect.value };
    }
    return next;
  },
  provide: (field) => EditorView.decorations.from(field, (value) => value.inline),
});

let shown = false;

/** Whether the blame gutter is shown, in every buffer. */
export function blameShown(): boolean {
  return shown;
}

export function toggleBlameShown(): void {
  shown = !shown;
}

const lineCommits = new WeakMap<Blame, (string | null)[]>();
function commitsOf(blame: Blame): (string | null)[] {
  let commits = lineCommits.get(blame);
  if (!commits) {
    commits = commitsByLine(blame);
    lineCommits.set(blame, commits);
  }
  return commits;
}

/**
 * Maps buffer lines to lines on disk, which blame describes, through the
 * unsaved edits against `saved`. Null when the edits are too large to map.
 */
export function toDisk(state: EditorState, saved: Text | null, dirty: boolean): ToDisk | null {
  if (!saved || !dirty) return (line) => line;
  if (saved.lines > MAX_DIFF_LINES || state.doc.lines > MAX_DIFF_LINES) return null;
  const hunks = diffLines(saved.toJSON(), state.doc.toJSON());
  return (line) => mapLine(hunks, line);
}

/** The effect that fills the blame gutter or empties it (`blame` null); null when nothing changes. */
export function blameGutterEffect(
  state: EditorState,
  blame: Blame | null,
  map: ToDisk | null,
): StateEffect<unknown> | null {
  if (!blame && state.field(blameField).gutter.size === 0) return null;
  if (!blame) return setBlameGutter.of(RangeSet.empty);
  const now = Date.now();
  const doc = state.doc;
  // The empty line after a final line break is not a line of code: it gets no label.
  const lines = isTrailingEmpty(doc, doc.lines) ? doc.lines - 1 : doc.lines;
  const blocks = blameBlocks(blame, lines, map ?? (() => null));
  return setBlameGutter.of(
    RangeSet.of(blocks.map((b) => new BlameMarker(blockLabel(b.commit, now)).range(doc.line(b.line + 1).from))),
  );
}

/** Whether line `number` (1-based) is the empty line after the file's final line break. */
function isTrailingEmpty(doc: Text, number: number): boolean {
  return number > 1 && number === doc.lines && doc.line(number).length === 0;
}

/** The blame of the cursor line, such as "Ann, 2 days ago · Start"; null on the empty last line. */
export function cursorBlame(state: EditorState, blame: Blame, map: ToDisk): string | null {
  const line = state.doc.lineAt(state.selection.main.head);
  if (isTrailingEmpty(state.doc, line.number)) return null;
  return lineBlame(blame, commitsOf(blame), map(line.number - 1), Date.now());
}

/** The effect that shows the inline blame of the cursor line; null when the line has none. */
export function inlineBlameEffect(state: EditorState, blame: Blame, map: ToDisk): StateEffect<unknown> | null {
  const text = cursorBlame(state, blame, map);
  if (text === null) return null;
  const line = state.doc.lineAt(state.selection.main.head);
  const widget = new InlineBlame(text);
  return setInlineBlame.of(Decoration.set([Decoration.widget({ widget, side: 1 }).range(line.to)]));
}

const styles = EditorView.baseTheme({
  // At least 4.5:1 on the editor's backgrounds (#16161a, active line #1d1d23).
  ".cm-git-blame-gutter .cm-gutterElement": { padding: "0 8px", color: "#9a9aa6" },
  ".cm-git-blame": { display: "inline-block", maxWidth: "22ch", overflow: "hidden", textOverflow: "ellipsis" },
  // Upright and after a rule, so it does not read as a comment in the code.
  ".cm-git-inline-blame": {
    marginLeft: "2em",
    paddingLeft: "0.75em",
    borderLeft: "1px solid #3a3a44",
    color: "#8a8a96",
  },
});

/** The blame state, its gutter (left of the line numbers) and styles. */
export const blameExtension = [
  blameField,
  Prec.high(gutter({ class: "cm-git-blame-gutter", markers: (view) => view.state.field(blameField).gutter })),
  styles,
];
