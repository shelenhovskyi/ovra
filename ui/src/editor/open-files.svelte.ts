// The open files of a project, shown as tabs and kept in the project state
// so the active file comes back when the project is reopened. Writes are
// partial (`open_files` and `active_file` only), so fields of other panels,
// like the tree's expanded folders, stay as they are. Each tab may have an
// editor buffer; a tab with unsaved text is never closed without asking.

import { tick, untrack } from "svelte";
import type { ProjectId, ProjectStatePatch } from "../lib/protocol";
import type { Transport } from "../lib/transport";
import type { Workspace } from "../shell/workspace.svelte";
import { Buffers } from "./buffers.svelte";
import { drafts } from "./drafts.svelte";
import { highlighter } from "./highlight/highlighter";

/** Beyond this many tabs, the leftmost inactive tab without unsaved text is closed. */
const MAX_OPEN_FILES = 20;

export class OpenFiles {
  /** Tab order. */
  files = $state<string[]>([]);
  active = $state<string | null>(null);
  private writes = Promise.resolve();

  constructor(
    private readonly transport: Transport,
    readonly project: ProjectId,
    readonly buffers: Buffers,
  ) {}

  /** Loads the stored state; resolves to the file that was active. */
  async restore(): Promise<string | null> {
    const { data: state } = await this.transport.call({
      method: "projects.get_state",
      params: { id: this.project },
    });
    this.files = state.open_files;
    this.active = state.active_file;
    return state.active_file;
  }

  /** Makes `path` (or nothing) the active file, adding a tab at the end if needed. */
  opened(path: string | null): Promise<void> {
    if (path === this.active) return this.writes;
    this.active = path;
    if (path && !this.files.includes(path)) {
      const files = [...this.files, path];
      for (let i = 0; files.length > MAX_OPEN_FILES && i < files.length; ) {
        const candidate = files[i] as string;
        if (candidate === path || this.buffers.isDirty(candidate)) {
          i++;
        } else {
          files.splice(i, 1);
          this.buffers.discard(candidate);
        }
      }
      this.files = files;
    }
    return this.write();
  }

  /** Removes a tab and its buffer; returns the file to show next. */
  close(path: string): string | null {
    const index = this.files.indexOf(path);
    if (index === -1) return this.active;
    this.buffers.discard(path);
    this.files = this.files.filter((p) => p !== path);
    if (this.active === path) this.active = this.files[index] ?? this.files[index - 1] ?? null;
    void this.write();
    return this.active;
  }

  /** The tab `offset` places away from the active one, wrapping around. */
  neighbour(offset: number): string | null {
    if (this.files.length === 0) return null;
    const index = this.active ? this.files.indexOf(this.active) : -1;
    const count = this.files.length;
    return this.files[(((index + offset) % count) + count) % count] ?? null;
  }

  private write(): Promise<void> {
    const state: ProjectStatePatch = { open_files: this.files, active_file: this.active };
    this.writes = this.writes
      .then(async () => {
        await this.transport.call({ method: "projects.set_state", params: { id: this.project, state } });
      })
      .catch(() => {
        // Losing the list of open files is harmless; the next change writes it again.
      });
    return this.writes;
  }
}

/**
 * The open project's tabs, or null before its state is loaded, and the tab
 * waiting for a Save / Don't save / Cancel answer.
 */
export const tabs = $state<{ current: OpenFiles | null; closing: string | null; notice: string | null }>({
  current: null,
  closing: null,
  notice: null,
});

type Target = Pick<Workspace, "openPath" | "mode" | "openFile">;

/** Unsaved editor text for a file of the open project, or null. Reactive; for the viewer. */
export function unsavedText(project: ProjectId, path: string): string | null {
  const current = tabs.current;
  return current?.project === project ? current.buffers.unsavedText(path) : null;
}

/** Closes a tab, first asking (via `tabs.closing`) if it has unsaved text. */
export function requestClose(workspace: Target, path: string): void {
  const current = tabs.current;
  if (!current) return;
  if (current.buffers.isDirty(path)) {
    tabs.closing = path;
    return;
  }
  closeTab(workspace, current, path);
}

/** Answers the question for `tabs.closing`. */
export async function answerClose(workspace: Target, answer: "save" | "discard" | "cancel"): Promise<void> {
  const current = tabs.current;
  const path = tabs.closing;
  tabs.closing = null;
  if (!current || !path || answer === "cancel") return;
  if (answer === "save") {
    const saved = await current.buffers.open(path).save();
    if (!saved) {
      // Show why it failed (conflict or error) instead of closing.
      show(workspace, path, "edit");
      return;
    }
  }
  closeTab(workspace, current, path);
}

function closeTab(workspace: Target, current: OpenFiles, path: string): void {
  const wasActive = current.active === path;
  const next = current.close(path);
  if (wasActive) show(workspace, next);
  void keepFocus(current.files.length === 0);
}

/**
 * After a tab closed, focus must not stay on the page body: with no tab left
 * it goes to the file tree (where the file is still selected), otherwise
 * into the main area.
 */
async function keepFocus(lastTab: boolean): Promise<void> {
  await tick();
  const active = document.activeElement;
  if (active && active !== document.body && active.isConnected) return;
  const tree = document.querySelector<HTMLElement>('[role="tree"]');
  if (lastTab && tree) tree.focus();
  else await focusMain();
}

/**
 * Moves focus into the main area (the viewer's content, or the panel itself)
 * once it has rendered, so it never drops to the page body.
 */
export async function focusMain(): Promise<void> {
  await tick();
  const panel = document.getElementById("ovra-tabpanel");
  const candidates = [...(panel?.querySelectorAll<HTMLElement>(".cm-content, [data-panel] [tabindex='-1']") ?? [])];
  const target = candidates.find((element) => element.offsetParent !== null) ?? panel;
  target?.focus();
}

/** Shows the file at `path` (or nothing), keeping the current mode unless one is given. */
export function show(workspace: Target, path: string | null, mode = workspace.mode): void {
  if (path) workspace.openFile(path, mode);
  else workspace.openPath = null;
}

/** Keeps `tabs.current` in step with the open project; returns a function that stops. */
export function persistOpenFiles(transport: Transport, workspace: Workspace): () => void {
  return $effect.root(() => {
    $effect(() => {
      const project = workspace.project?.id;
      tabs.current = null;
      tabs.closing = null;
      if (!project) return;
      const buffers = new Buffers({ transport, project, drafts, highlighter });
      const files = new OpenFiles(transport, project, buffers);
      let live = true;
      const ready = () => {
        if (!live) return;
        tabs.current = files;
        void files.opened(workspace.openPath);
      };
      files.restore().then((active) => {
        if (live && active && workspace.openPath === null) workspace.openFile(active);
        ready();
      }, ready);
      // Unsaved text goes to drafts before the page goes away; only ask if that failed.
      const unload = (event: BeforeUnloadEvent) => {
        if (!buffers.flushDrafts()) event.preventDefault();
      };
      const hide = () => buffers.flushDrafts();
      window.addEventListener("beforeunload", unload);
      window.addEventListener("pagehide", hide);
      return () => {
        live = false;
        window.removeEventListener("beforeunload", unload);
        window.removeEventListener("pagehide", hide);
        buffers.close();
      };
    });

    $effect(() => {
      const path = workspace.openPath;
      const current = tabs.current;
      untrack(() => {
        tabs.notice = null;
        void current?.opened(path);
      });
    });
  });
}
