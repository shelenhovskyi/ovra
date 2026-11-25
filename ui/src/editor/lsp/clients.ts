// Language servers for one project's editor buffers (docs/specs/lsp.md,
// ADR 0007). A server starts only because a person showed a file of its
// language in the editor, and only for a trusted project with the server
// installed. One LSPClient per running server; a server that stops or fails
// drops its client, and the next file shown starts a new one.

import { lintGutter, setDiagnostics } from "@codemirror/lint";
import { findReferences, LSPClient, LSPPlugin, languageServerExtensions } from "@codemirror/lsp-client";
import { Prec, Text } from "@codemirror/state";
import { EditorView, keymap, ViewPlugin } from "@codemirror/view";
import type { KnownServer, LspStatus, ProjectId, ServerId, ServerInfo } from "../../lib/protocol";
import type { Transport } from "../../lib/transport";
import { languageServers } from "../../lsp/servers.svelte";
import type { Buffer, Buffers } from "../buffers.svelte";
import { languageFor } from "../languages";
import { actionKeys, keyboardHover, type LspAction, runAction } from "./actions";
import { ServerChannel } from "./channel";
import { renameAcross } from "./rename";
import { safeHref, sanitizeHtml } from "./sanitize";
import { pathOf, ROOT_URI, uriOf } from "./uris";
import { BufferWorkspace } from "./workspace";

/** Editor language (folder in `languages/`) to server language and LSP `languageId`. */
const LANGUAGES: Record<string, { server: string; languageId: string }> = {
  css: { server: "css", languageId: "css" },
  javascript: { server: "typescript", languageId: "javascript" },
  json: { server: "json", languageId: "json" },
  python: { server: "python", languageId: "python" },
  rust: { server: "rust", languageId: "rust" },
  tsx: { server: "typescript", languageId: "typescriptreact" },
  typescript: { server: "typescript", languageId: "typescript" },
};

/** The server language for `path` (`typescript` for a `.tsx` file), or null. */
export function serverLanguageFor(path: string): string | null {
  const language = languageFor(path);
  return (language && LANGUAGES[language.id]?.server) ?? null;
}

/** The installed server that would serve `path` in a trusted project, or null. */
export function serverFor(path: string, status: Pick<LspStatus, "trusted" | "known"> | null): KnownServer | null {
  const language = languageFor(path);
  const server = language ? LANGUAGES[language.id]?.server : undefined;
  if (!server || !status?.trusted) return null;
  return status.known.find((k) => k.language === server && k.path !== null) ?? null;
}

/** Files not shown in the editor whose text is read to list references in them. */
const MAX_PEEKED = 50;

/** Servers indexing a large project answer slowly; the library's default is 3 s. */
const REQUEST_TIMEOUT_MS = 10_000;

type Options = {
  transport: Transport;
  project: ProjectId;
  /** The project's name, for the root folder in `initialize`. */
  name: string;
  /** Shows a project file in the editor. */
  open: (path: string) => void;
  /** Tells the person something short, such as why a jump did nothing. */
  note: (text: string) => void;
  /** The project's buffers, where a rename changes files the editor does not show. */
  buffers: Buffers;
  /** Adds a tab for a file without showing it. */
  keep: (path: string) => void;
};

type Server = { language: string; info: ServerInfo; client: LSPClient; channel: ServerChannel; buffers: Set<Buffer> };

/** Minimal `WorkspaceEdit`: the library applies only `changes`. */
type WorkspaceEdit = { changes?: Record<string, unknown[]> } | null;

type Location = { uri: string };

/** Where each client's errors go: the library would show a bar per error that Escape cannot close. */
const notes = new WeakMap<LSPClient, (text: string) => void>();
LSPPlugin.prototype.reportError = function (this: LSPPlugin, text: string, err: unknown) {
  const line = `${text}: ${err instanceof Error ? err.message : String((err as { message?: unknown })?.message ?? err)}`;
  const note = notes.get(this.client);
  if (note) note(line);
  else console.warn(`[lsp] ${line}`);
};

/** Hover-doc links: readable, and they say where they go. */
const linkTheme = EditorView.baseTheme({
  ".cm-lsp-documentation a": { color: "var(--accent)", textDecoration: "underline" },
});

export class LanguageClients {
  private readonly servers = new Map<string, Server>();
  private readonly starting = new Map<string, Promise<Server | null>>();
  private status: Promise<LspStatus | null> | null = null;
  private stopping: Promise<unknown> = Promise.resolve();
  private shown: { buffer: Buffer; view: EditorView } | null = null;
  private waiters: { path: string; resolve: (view: EditorView | null) => void }[] = [];
  private closed = false;
  private toldUntrusted = false;
  private readonly unsubscribe: () => void;

  constructor(private readonly options: Options) {
    this.unsubscribe = options.transport.onEvent((event) => {
      if (event.type === "lsp.message") {
        this.byId(event.data.server)?.channel.deliver(event.data.message);
      } else if (event.type === "lsp.changed") {
        const { id, state } = event.data.server;
        const server = this.byId(id);
        if (server && (state === "stopped" || state === "failed")) this.drop(server);
      } else if (event.type === "info.resync") {
        this.restart();
      }
    });
  }

  /** The editor now shows `buffer` in `view`: connect it to its language's server, starting one if needed. */
  showing(buffer: Buffer, view: EditorView): void {
    this.shown = { buffer, view };
    const ready = buffer.status === "ready";
    for (const waiter of this.waiters.splice(0)) waiter.resolve(ready && waiter.path === buffer.path ? view : null);
    if (ready) void this.connect(buffer);
  }

  /** The editor stopped showing `buffer`. */
  hiding(buffer: Buffer): void {
    if (this.shown?.buffer === buffer) this.shown = null;
  }

  /** Runs a language-server action in the shown file, or says why it cannot. */
  action(action: LspAction): void {
    const view = this.shown?.view;
    const language = this.shown?.buffer.language;
    if (
      !view ||
      !runAction(
        view,
        action,
        this.context(() => LSPPlugin.get(view)?.client ?? null),
      )
    ) {
      this.options.note(
        view && language && LANGUAGES[language.id]
          ? "No language server is running for this file."
          : "Open a file for editing (Mod+E) to use language features.",
      );
    }
  }

  private context(client: () => LSPClient | null) {
    return {
      note: this.options.note,
      references: (view: EditorView) => this.references(view),
      serverName: () => {
        const mine = client();
        for (const server of this.servers.values()) if (server.client === mine) return server.info.name;
        return "This language server";
      },
    };
  }

  /** Forgets every client. Servers keep running (a reload does not restart them). */
  close(): void {
    this.closed = true;
    this.unsubscribe();
    for (const server of [...this.servers.values()]) this.drop(server);
    for (const waiter of this.waiters.splice(0)) waiter.resolve(null);
  }

  private async connect(buffer: Buffer): Promise<void> {
    const language = buffer.language ? LANGUAGES[buffer.language.id] : undefined;
    if (!language) return;
    const server = await this.server(language.server);
    if (!server || server.buffers.has(buffer)) return;
    server.buffers.add(buffer);
    buffer.setLanguageServer(server.client.plugin(uriOf(buffer.path), language.languageId));
  }

  private server(language: string): Promise<Server | null> {
    const running = this.servers.get(language);
    if (running) return Promise.resolve(running);
    let pending = this.starting.get(language);
    if (!pending) {
      pending = this.start(language).finally(() => this.starting.delete(language));
      this.starting.set(language, pending);
    }
    return pending;
  }

  private async start(language: string): Promise<Server | null> {
    await this.stopping;
    const status = await this.lspStatus();
    const known = status?.known.find((k) => k.language === language);
    // A known server that cannot start gets the footer's hint.
    if (status && known && (!status.trusted || !known.path))
      languageServers(this.options.transport).needServer(language);
    if (status && known?.path && !status.trusted && !this.toldUntrusted) {
      this.toldUntrusted = true;
      this.options.note(
        `No ${known.name} here: language servers run the project's code, so they start only once you trust the project (Trust project, in the Agents panel).`,
      );
    }
    if (!status?.trusted || !known?.path || this.closed) return null;
    let info = await this.startServer(language);
    if (info && status.servers.some((s) => s.id === info?.id)) {
      // Left running by an earlier page (a reload) and initialized by its
      // client; a server cannot be initialized twice.
      await this.stopServer(info.id);
      info = await this.startServer(language);
    }
    if (!info || this.closed) return null;
    const client: LSPClient = new LSPClient({
      rootUri: ROOT_URI,
      timeout: REQUEST_TIMEOUT_MS,
      workspace: (c) => new BufferWorkspace(c, (uri) => this.display(uri)),
      sanitizeHTML: sanitizeHtml,
      extensions: [
        ...languageServerExtensions(),
        { clientCapabilities: { workspace: { workspaceFolders: true } } },
        this.docLinks(),
        Prec.high(keymap.of(actionKeys(this.context(() => client)))),
        keyboardHover,
        lintGutter(),
        linkTheme,
      ],
    });
    const channel = new ServerChannel(this.options.transport, info.id, {
      folderName: this.options.name,
      renamed: (result) => this.renamed(client, result as WorkspaceEdit),
    });
    notes.set(client, this.options.note);
    const server: Server = { language, info, client, channel, buffers: new Set() };
    this.servers.set(language, server);
    client.connect(channel);
    return server;
  }

  private async startServer(language: string): Promise<ServerInfo | null> {
    try {
      const { data } = await this.options.transport.call({
        method: "lsp.start",
        params: { project: this.options.project, language },
      });
      return data;
    } catch (err) {
      console.warn(`[lsp] ${language} server did not start:`, err);
      return null;
    }
  }

  private stopServer(server: ServerId): Promise<void> {
    return this.options.transport.call({ method: "lsp.stop", params: { server } }).then(
      () => {},
      (err: unknown) => console.warn("[lsp] stop failed:", err),
    );
  }

  /** `lsp.status`, asked once per project; asked again while the project is untrusted, since trust can come later. */
  private lspStatus(): Promise<LspStatus | null> {
    this.status ??= this.options.transport
      .call({ method: "lsp.status", params: { project: this.options.project } })
      .then(
        ({ data }) => data,
        (err: unknown) => {
          console.warn("[lsp] no status:", err);
          return null;
        },
      );
    const asked = this.status;
    return asked.then((status) => {
      if (!status?.trusted && this.status === asked) this.status = null;
      return status;
    });
  }

  private byId(id: ServerId): Server | undefined {
    for (const server of this.servers.values()) if (server.info.id === id) return server;
    return undefined;
  }

  private drop(server: Server): void {
    if (this.servers.get(server.language) !== server) return;
    this.servers.delete(server.language);
    server.channel.close();
    server.client.disconnect();
    for (const buffer of server.buffers) buffer.setLanguageServer([], (state) => setDiagnostics(state, []));
  }

  /**
   * Messages were lost (a reconnect or a backed-up event channel), and a
   * client cannot initialize the same server twice: stop every server and
   * start the shown file's one again.
   */
  private restart(): void {
    const servers = [...this.servers.values()];
    this.status = null;
    if (servers.length === 0) return;
    this.stopping = Promise.all(
      servers.map((server) => {
        this.drop(server);
        return this.stopServer(server.info.id);
      }),
    );
    if (this.shown) void this.connect(this.shown.buffer);
  }

  /** Shows the file a location names; files outside the project are never opened. */
  private display(uri: string): Promise<EditorView | null> {
    const path = pathOf(uri);
    if (path === null) {
      this.options.note(`${outsideName(uri)} is outside the project and cannot be opened.`);
      return Promise.resolve(null);
    }
    if (this.shown?.buffer.path === path) return Promise.resolve(this.shown.view);
    return new Promise((resolve) => {
      this.waiters.push({ path, resolve });
      this.options.open(path);
    });
  }

  /**
   * The editor holds one file open at a time, and the library applies a
   * rename only to open files: a rename that also changes other files is
   * taken from the library and applied by `renameAcross`, to every file or
   * to none, as unsaved edits the person reviews.
   */
  private renamed(client: LSPClient, edit: WorkspaceEdit): WorkspaceEdit {
    const workspace = client.workspace as BufferWorkspace;
    const open = new Set(workspace.open().map((file) => file.uri));
    const changes = edit?.changes ?? {};
    const { buffers, keep, note } = this.options;
    // Files the server reads from disk while they hold other text: it renamed
    // without seeing it, so the rename would miss what is only there.
    const hidden = this.unsavedHidden(client);
    if (hidden.length > 0) {
      note(`Rename not applied: save or revert ${hidden.join(", ")} first; the server only sees saved text there.`);
      return null;
    }
    if (Object.entries(changes).every(([uri, edits]) => edits.length === 0 || open.has(uri))) return edit;
    void renameAcross(changes, { workspace, buffers, keep, note }).catch((err: unknown) =>
      note(`Rename not applied: ${err instanceof Error ? err.message : String(err)}`),
    );
    return null;
  }

  /** Files of `client`'s language with unsaved text that the server is not shown. */
  private unsavedHidden(client: LSPClient): string[] {
    const language = [...this.servers.values()].find((s) => s.client === client)?.language;
    const open = new Set((client.workspace as BufferWorkspace).open().map((file) => pathOf(file.uri)));
    return this.options.buffers.unsavedPaths().filter((path) => {
      const editor = languageFor(path);
      return !open.has(path) && editor !== null && LANGUAGES[editor.id]?.server === language;
    });
  }

  /**
   * Finds references in every project file, not only the shown one: the
   * library lists references only in files of its workspace, so the
   * files the answer names are read from disk and peeked first, then the
   * library asks again and shows the list.
   */
  private references(view: EditorView): boolean {
    const plugin = LSPPlugin.get(view);
    if (!plugin) return false;
    const workspace = plugin.client.workspace as BufferWorkspace;
    workspace.unpeek();
    plugin.client.sync();
    plugin.client
      .request<unknown, Location[] | null>("textDocument/references", {
        textDocument: { uri: plugin.uri },
        position: plugin.toPosition(view.state.selection.main.head),
        context: { includeDeclaration: true },
      })
      .then(async (locations) => {
        const uris = [...new Set((locations ?? []).map((l) => l.uri))].filter((uri) => !workspace.getFile(uri));
        const read = uris.slice(0, MAX_PEEKED);
        const texts = await Promise.all(read.map((uri) => this.diskText(uri)));
        for (const [i, uri] of read.entries()) {
          const text = texts[i];
          if (typeof text === "string") workspace.peek(uri, Text.of(text.split(/\r\n?|\n/)));
        }
        if (LSPPlugin.get(view) !== plugin) return;
        findReferences(view);
        const hidden = this.unsavedHidden(plugin.client);
        if (hidden.length > 0)
          this.options.note(`Not listed: references only in the unsaved text of ${hidden.join(", ")}.`);
      })
      .catch((err: unknown) => plugin.reportError("Finding references failed", err));
    return true;
  }

  /** A project file's text as saved, or null for a file outside the project or not text. */
  private async diskText(uri: string): Promise<string | null> {
    const path = pathOf(uri);
    if (path === null) return null;
    try {
      const { data } = await this.options.transport.call({
        method: "fs.read_file",
        params: { project: this.options.project, path },
      });
      return data.body.kind === "text" ? data.body.text : null;
    } catch {
      return null;
    }
  }

  /** Links in hover and completion docs open in the system browser, never in this window. */
  private docLinks() {
    const transport = this.options.transport;
    return ViewPlugin.define((view) => {
      const click = (event: MouseEvent) => {
        const link = event.target instanceof Element ? event.target.closest("a") : null;
        if (!link || !view.dom.contains(link)) return;
        event.preventDefault();
        const url = safeHref(link.getAttribute("href"));
        if (url && event.type === "click") {
          transport.call({ method: "open.url", params: { url } }).catch((err: unknown) => {
            console.warn("[lsp] link not opened:", err);
          });
        }
      };
      view.dom.addEventListener("click", click, true);
      view.dom.addEventListener("auxclick", click, true);
      return {
        destroy() {
          view.dom.removeEventListener("click", click, true);
          view.dom.removeEventListener("auxclick", click, true);
        },
      };
    });
  }
}

/** A readable name for a URI outside the project: the whole path of a file, so `/etc/passwd` is not just "passwd". */
function outsideName(uri: string): string {
  try {
    const url = new URL(uri);
    if (url.protocol !== "file:") return "A location";
    return decodeURIComponent(url.pathname) || uri;
  } catch {
    return "A location";
  }
}
