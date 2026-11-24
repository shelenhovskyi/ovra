// Editor side of the highlight worker: numbered documents and requests.

import type { FromWorker, LanguageSource, Port, Span, TextChange, ToWorker } from "./protocol";

export class HighlightClient {
  private nextDoc = 1;
  private nextRequest = 1;
  private readonly pending = new Map<number, { resolve: (spans: Span[]) => void; reject: (err: Error) => void }>();

  constructor(private readonly port: Port<FromWorker, ToWorker>) {
    port.onmessage = ({ data: reply }) => {
      const waiter = this.pending.get(reply.id);
      if (!waiter) return;
      this.pending.delete(reply.id);
      if (reply.type === "spans") waiter.resolve(reply.spans);
      else waiter.reject(new Error(reply.message));
    };
  }

  open(language: LanguageSource, text: string): HighlightedDoc {
    const doc = this.nextDoc++;
    this.port.postMessage({ type: "open", doc, language, text });
    return {
      edit: (changes) => this.port.postMessage({ type: "edit", doc, changes }),
      highlight: (from, to) =>
        new Promise((resolve, reject) => {
          const id = this.nextRequest++;
          this.pending.set(id, { resolve, reject });
          this.port.postMessage({ type: "highlight", id, doc, from, to });
        }),
      close: () => this.port.postMessage({ type: "close", doc }),
    };
  }
}

/** One document open in the worker. */
export type HighlightedDoc = {
  edit(changes: TextChange[]): void;
  highlight(from: number, to: number): Promise<Span[]>;
  close(): void;
};
