// The shared highlight worker, started on first use.

import { HighlightClient } from "./client";

let client: HighlightClient | null = null;

export function highlighter(): HighlightClient {
  client ??= new HighlightClient(new Worker(new URL("./worker.ts", import.meta.url), { type: "module" }));
  return client;
}
