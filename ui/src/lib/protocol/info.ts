export type InfoRequest = { method: "info.get" };

export type CoreInfo = { name: string; version: string; mode: string };

export type InfoResponse = { type: "info"; data: CoreInfo };

export type InfoEvent = { type: "info.ready" };
