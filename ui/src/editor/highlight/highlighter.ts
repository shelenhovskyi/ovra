// The shared highlight worker, started on first use.

import { HighlightClient } from "./client";

let client: HighlightClient | null = null;

/** The client, or null where workers are unavailable (unit tests). */
export function highlighter(): HighlightClient | null {
  if (typeof Worker === "undefined") return null;
  client ??= new HighlightClient(new Worker(new URL("./worker.ts", import.meta.url), { type: "module" }));
  return client;
}
