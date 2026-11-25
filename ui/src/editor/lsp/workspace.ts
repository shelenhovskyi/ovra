// The language client's view of the editor's files. The editor shows one
// buffer at a time in one EditorView, and each buffer's state carries the
// client's plugin, so a file is open towards the server exactly while its
// buffer is attached to the view: attaching creates the plugin (`openFile`),
// `view.setState` for another buffer destroys it (`closeFile`).

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

export class BufferWorkspace extends Workspace {
  files: OpenFile[] = [];
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
    for (const file of this.files) {
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
    const known = this.getFile(uri) as OpenFile | null;
    if (known) {
      known.view = view;
      return;
    }
    const file = new OpenFile(uri, languageId, this.nextVersion(uri), view.state.doc, view);
    this.files.push(file);
    this.client.didOpen(file);
  }

  closeFile(uri: string, view: EditorView): void {
    const file = this.getFile(uri) as OpenFile | null;
    if (!file || file.view !== view) return;
    this.files = this.files.filter((f) => f !== file);
    this.client.didClose(uri);
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
