// Ranking off the main thread: a long list (the project's files) is ranked
// in a Web Worker so typing never waits for it; short lists rank in place.

import { rankAll } from "./fuzzy";

type RankRow = { index: number; positions: number[] };
type RankResult = { rows: RankRow[]; matches: number };

export interface Ranker {
  rank(query: string, limit: number): Promise<RankResult>;
  dispose(): void;
}

type ToWorker = { labels: string[] } | { id: number; query: string; limit: number };
type FromWorker = { id: number } & RankResult;

/** The message side of a Worker or a MessagePort. */
export interface Port {
  postMessage(message: unknown): void;
  addEventListener(type: "message", listener: (event: MessageEvent) => void): void;
}

function rankLabels(labels: string[], indices: number[], query: string, limit: number): RankResult {
  const { rows, matches } = rankAll(indices, (i) => labels[i] ?? "", query, limit);
  return { rows: rows.map(({ item, positions }) => ({ index: item, positions })), matches };
}

/** Worker side: answers rank requests over `port`. */
export function serveRanker(port: Port): void {
  let labels: string[] = [];
  let indices: number[] = [];
  port.addEventListener("message", (event) => {
    const message = event.data as ToWorker;
    if ("labels" in message) {
      labels = message.labels;
      indices = labels.map((_, i) => i);
      return;
    }
    port.postMessage({ id: message.id, ...rankLabels(labels, indices, message.query, message.limit) });
  });
}

/** Main-thread side over `port`; answers arrive in order, so each request waits for its own id. */
export function remoteRanker(port: Port, labels: string[], dispose: () => void = () => {}): Ranker {
  let nextId = 1;
  const waiting = new Map<number, (result: RankResult) => void>();
  port.addEventListener("message", (event) => {
    const { id, ...result } = event.data as FromWorker;
    waiting.get(id)?.(result);
    waiting.delete(id);
  });
  port.postMessage({ labels } satisfies ToWorker);
  return {
    rank: (query, limit) =>
      new Promise((resolve) => {
        const id = nextId++;
        waiting.set(id, resolve);
        port.postMessage({ id, query, limit } satisfies ToWorker);
      }),
    dispose,
  };
}

const IN_PLACE_MAX = 2_000;

export function createRanker(labels: string[]): Ranker {
  if (labels.length > IN_PLACE_MAX && typeof Worker !== "undefined") {
    const worker = new Worker(new URL("./rank.worker.ts", import.meta.url), { type: "module" });
    return remoteRanker(worker, labels, () => worker.terminate());
  }
  const indices = labels.map((_, i) => i);
  return { rank: async (query, limit) => rankLabels(labels, indices, query, limit), dispose: () => {} };
}
