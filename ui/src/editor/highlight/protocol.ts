// Messages between the editor and the highlight worker. Positions are
// UTF-16 offsets, as in CodeMirror and web-tree-sitter.

/** A grammar (URL or bytes) and its highlight queries, earliest wins. */
export type LanguageSource = { id: string; grammar: string | Uint8Array; queries: string[] };

/** One edit; a list of them applies in order, each against the previous result. */
export type TextChange = { from: number; to: number; insert: string };

/** A highlighted range and its CSS classes. */
export type Span = [from: number, to: number, classes: string];

export type ToWorker =
  | { type: "open"; doc: number; language: LanguageSource; text: string }
  | { type: "edit"; doc: number; changes: TextChange[] }
  | { type: "highlight"; id: number; doc: number; from: number; to: number }
  | { type: "close"; doc: number };

export type FromWorker = { type: "spans"; id: number; spans: Span[] } | { type: "failed"; id: number; message: string };

/** The part of `Worker` / `DedicatedWorkerGlobalScope` / `MessagePort` we use. */
export type Port<In, Out> = {
  postMessage(message: Out): void;
  onmessage: ((event: MessageEvent<In>) => void) | null;
};
