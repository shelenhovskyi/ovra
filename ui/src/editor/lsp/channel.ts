// The wire between one LSPClient and one language server: `lsp.send` out,
// `lsp.message` events in, both over Ovra's own transport (ADR 0007).

import type { Transport as LspTransport } from "@codemirror/lsp-client";
import type { JsonRpcMessage, ServerId } from "../../lib/protocol";
import type { Transport } from "../../lib/transport";
import { ROOT_URI } from "./uris";

type Hooks = {
  /** Shown as the root folder's name in `initialize`. */
  folderName: string;
  /** Gets every `textDocument/rename` result and returns the one to apply. */
  renamed: (result: unknown) => unknown;
};

export class ServerChannel implements LspTransport {
  private readonly handlers = new Set<(value: string) => void>();
  private readonly renames = new Set<unknown>();
  private closed = false;

  constructor(
    private readonly core: Transport,
    readonly server: ServerId,
    private readonly hooks: Hooks,
  ) {}

  send(text: string): void {
    if (this.closed) return;
    const message = JSON.parse(text) as JsonRpcMessage;
    if (message.method === "initialize" && isRecord(message.params)) {
      message.params.workspaceFolders = [{ uri: ROOT_URI, name: this.hooks.folderName }];
    }
    if (message.method === "textDocument/rename" && "id" in message) this.renames.add(message.id);
    // The server may have stopped meanwhile; `lsp.changed` tells the client manager.
    this.core.call({ method: "lsp.send", params: { server: this.server, message } }).catch((err: unknown) => {
      console.warn(`[lsp] ${String(message.method ?? "response")} not sent:`, err);
    });
  }

  /** Hands one message from the server to the client. */
  deliver(message: JsonRpcMessage): void {
    if (this.closed) return;
    if (!("method" in message) && this.renames.delete(message.id) && "result" in message) {
      message = { ...message, result: this.hooks.renamed(message.result) };
    }
    const text = JSON.stringify(message);
    for (const handler of this.handlers) handler(text);
  }

  subscribe(handler: (value: string) => void): void {
    this.handlers.add(handler);
  }

  unsubscribe(handler: (value: string) => void): void {
    this.handlers.delete(handler);
  }

  /** Drops everything from now on, in both directions. */
  close(): void {
    this.closed = true;
    this.handlers.clear();
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
