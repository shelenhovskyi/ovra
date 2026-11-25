// The language client's view of the editor's files. The editor shows one
// buffer at a time in one EditorView, and each buffer's state carries the
// client's plugin, so a file is open towards the server exactly while its
// buffer is attached to the view: attaching creates the plugin (`openFile`),
// `view.setState` for another buffer destroys it (`closeFile`).
//
// A file the editor does not show can also be "peeked": its text from disk
// is in `files` so the library can list references in it, but the server
// is never told it was opened. Showing it opens it for real.

import { type LSPClient, LSPPlugin, Workspace, type WorkspaceFile } from "@codemirror/lsp-client";
import type { Text } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";

/** Not exported by the library: what `syncFiles` returns. */
type WorkspaceFileUpdate = ReturnType<Workspace["syncFiles"]>[number];

class OpenFile implements WorkspaceFile {
  constructor(
    readonly uri: string,
    readonly languageId: string,
    public version: number,
    public doc: Text,
    public view: EditorView,
  ) {}

  getView(): EditorView {
    return this.view;
  }
}

class PeekedFile implements WorkspaceFile {
  readonly version = 0;
  constructor(
    readonly uri: string,
    readonly doc: Text,
  ) {}

  readonly languageId = "";

  getView(): null {
    return null;
  }
}

export class BufferWorkspace extends Workspace {
  files: (OpenFile | PeekedFile)[] = [];
  private readonly versions = new Map<string, number>();

  constructor(
    client: LSPClient,
    /** Shows the file named by a URI in the editor; null if it cannot be shown. */
    private readonly display: (uri: string) => Promise<EditorView | null>,
  ) {
    super(client);
  }

  syncFiles(): readonly WorkspaceFileUpdate[] {
    const updates: WorkspaceFileUpdate[] = [];
    for (const file of this.open()) {
      const plugin = LSPPlugin.get(file.view);
      if (!plugin || plugin.unsyncedChanges.empty) continue;
      updates.push({ file, prevDoc: file.doc, changes: plugin.unsyncedChanges });
      file.doc = file.view.state.doc;
      file.version = this.nextVersion(file.uri);
      plugin.clear();
    }
    return updates;
  }

  openFile(uri: string, languageId: string, view: EditorView): void {
    const known = this.getFile(uri);
    if (known instanceof OpenFile) {
      known.view = view;
      return;
    }
    if (known) this.files = this.files.filter((f) => f !== known);
    const file = new OpenFile(uri, languageId, this.nextVersion(uri), view.state.doc, view);
    this.files.push(file);
    this.client.didOpen(file);
  }

  closeFile(uri: string, view: EditorView): void {
    const file = this.getFile(uri);
    if (!(file instanceof OpenFile) || file.view !== view) return;
    this.files = this.files.filter((f) => f !== file);
    this.client.didClose(uri);
  }

  /** Only the files open towards the server: on a reconnect, peeked files stay unknown to it. */
  connected(): void {
    for (const file of this.open()) this.client.didOpen(file);
  }

  /** The files open towards the server, without peeked ones. */
  open(): OpenFile[] {
    return this.files.filter((f) => f instanceof OpenFile);
  }

  /** Adds a file's text from disk, unless the file is in the workspace already. */
  peek(uri: string, doc: Text): void {
    if (!this.getFile(uri)) this.files.push(new PeekedFile(uri, doc));
  }

  /** Drops every peeked file. */
  unpeek(): void {
    this.files = this.open();
  }

  displayFile(uri: string): Promise<EditorView | null> {
    return this.display(uri);
  }

  /** Versions keep rising across close and reopen, so a late diagnostic never matches new text. */
  private nextVersion(uri: string): number {
    const version = (this.versions.get(uri) ?? -1) + 1;
    this.versions.set(uri, version);
    return version;
  }
}
