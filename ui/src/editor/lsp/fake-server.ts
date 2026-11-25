// A fake core with a tiny fake language server behind `lsp.*`, for the
// client's tests: it answers `initialize`, records what the client sends,
// and answers requests from `answers`.

import type { JsonRpcMessage, KnownServer, Request, ServerInfo } from "../../lib/protocol";
import { CoreError } from "../../lib/transport";
import { FakeCore } from "../fake-core";

export class FakeLspCore extends FakeCore {
  trusted = true;
  known: KnownServer[] = [
    { language: "typescript", name: "typescript-language-server", path: "/bin/tsls", install: "npm i -g" },
    { language: "rust", name: "rust-analyzer", path: null, install: "rustup component add rust-analyzer" },
  ];
  /** Every `lsp.*` call, in order. */
  lspCalls: Request[] = [];
  /** Messages each server got, by server id. */
  received = new Map<string, JsonRpcMessage[]>();
  /** URLs asked to open in the system browser. */
  opened: string[] = [];
  /** Results for requests by method. */
  answers: Record<string, unknown> = {};
  private nextId = 1;
  private running = new Map<string, ServerInfo>();

  /** Messages of `method` that server `id` got. */
  got(id: string, method: string): JsonRpcMessage[] {
    return (this.received.get(id) ?? []).filter((m) => m.method === method);
  }

  /** The server sends a message to the client. */
  push(server: string, message: Omit<JsonRpcMessage, "jsonrpc">): void {
    this.emit({ type: "lsp.message", data: { server, message: { jsonrpc: "2.0", ...message } } });
  }

  /** The server exits. */
  stopServer(id: string): void {
    const info = this.running.get(id);
    if (!info) return;
    this.running.delete(id);
    this.emit({ type: "lsp.changed", data: { server: { ...info, state: "stopped" } } });
  }

  // biome-ignore lint/suspicious/noExplicitAny: test double for a generic method
  override async call(request: Request): Promise<any> {
    if (request.method === "open.url") {
      this.opened.push(request.params.url);
      return { type: "ok" };
    }
    if (!request.method.startsWith("lsp.")) return super.call(request);
    this.lspCalls.push(request);
    switch (request.method) {
      case "lsp.status":
        return {
          type: "lsp.status",
          data: { trusted: this.trusted, sandbox: null, servers: [...this.running.values()], known: this.known },
        };
      case "lsp.start": {
        const { project, language } = request.params;
        const running = [...this.running.values()].find((s) => s.language === language);
        if (running) return { type: "lsp.server", data: running };
        const info: ServerInfo = {
          id: `s${this.nextId++}`,
          project,
          language,
          name: "fake",
          sandbox: null,
          state: "running",
        };
        this.running.set(info.id, info);
        return { type: "lsp.server", data: info };
      }
      case "lsp.send": {
        const { server, message } = request.params;
        if (!this.running.has(server)) throw new CoreError({ code: "not_found", message: "no such server" });
        this.received.set(server, [...(this.received.get(server) ?? []), message]);
        if ("id" in message && typeof message.method === "string") this.answer(server, message);
        return { type: "ok" };
      }
      case "lsp.stop":
        this.stopServer(request.params.server);
        return { type: "ok" };
    }
  }

  private answer(server: string, message: JsonRpcMessage): void {
    const result =
      message.method === "initialize"
        ? {
            capabilities: {
              textDocumentSync: 2,
              hoverProvider: true,
              definitionProvider: true,
              renameProvider: true,
              referencesProvider: true,
            },
          }
        : (this.answers[message.method as string] ?? null);
    queueMicrotask(() => this.push(server, { id: message.id, result }));
  }
}
