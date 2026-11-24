export type InfoRequest = { method: "info.get" };

export type CoreInfo = { name: string; version: string; mode: string };

export type InfoResponse = { type: "info"; data: CoreInfo };

/**
 * `info.resync`: the client fell behind and missed `skipped` events, so
 * every panel reloads what it shows.
 */
export type InfoEvent = { type: "info.ready" } | { type: "info.resync"; data: { skipped: number } };
