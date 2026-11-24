// CodeMirror extension that paints highlight spans from the worker. Spans for
// the visible range are requested after every change and scroll; until they
// arrive, the previous ones are mapped through the edits.

import { type ChangeSet, StateEffect, StateField } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, ViewPlugin, type ViewUpdate } from "@codemirror/view";
import type { HighlightedDoc } from "./client";
import type { Span, TextChange } from "./protocol";

const setSpans = StateEffect.define<Span[]>();

const marks = new Map<string, Decoration>();

function mark(classes: string): Decoration {
  let decoration = marks.get(classes);
  if (!decoration) {
    decoration = Decoration.mark({ class: classes });
    marks.set(classes, decoration);
  }
  return decoration;
}

const spans = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(decorations, tr) {
    let next = decorations.map(tr.changes);
    for (const effect of tr.effects) {
      if (effect.is(setSpans)) {
        const length = tr.state.doc.length;
        next = Decoration.set(
          effect.value
            .filter(([from, to]) => to <= length && from < to)
            .map(([from, to, cls]) => mark(cls).range(from, to)),
          true,
        );
      }
    }
    return next;
  },
  provide: (field) => EditorView.decorations.from(field),
});

/** Converts a CodeMirror change set into sequential changes. */
function toChanges(changes: ChangeSet): TextChange[] {
  const out: TextChange[] = [];
  let shift = 0;
  changes.iterChanges((fromA, toA, _fromB, _toB, inserted) => {
    const insert = inserted.toString();
    out.push({ from: fromA + shift, to: toA + shift, insert });
    shift += insert.length - (toA - fromA);
  });
  return out;
}

export function treeSitterHighlighting(doc: HighlightedDoc) {
  const plugin = ViewPlugin.define((view) => {
    let version = 0;
    let destroyed = false;
    let last = "";
    const request = () => {
      const asked = version;
      const { from, to } = view.viewport;
      const key = `${asked}:${from}:${to}`;
      if (key === last) return;
      last = key;
      doc.highlight(from, to).then(
        (result) => {
          if (!destroyed && asked === version) view.dispatch({ effects: setSpans.of(result) });
        },
        () => {
          // Highlighting is best effort; the text stays plain.
        },
      );
    };
    request();
    return {
      update(update: ViewUpdate) {
        if (update.docChanged) {
          version++;
          doc.edit(toChanges(update.changes));
        }
        if (update.docChanged || update.viewportChanged) request();
      },
      destroy() {
        // The document belongs to the buffer, which outlives this view.
        destroyed = true;
      },
    };
  });
  return [spans, plugin];
}
