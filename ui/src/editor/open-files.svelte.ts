// The open files of a project, shown as tabs and kept in the project state
// so the active file comes back when the project is reopened. Writes are
// partial (`open_files`, `active_file` and `pinned_files` only), so fields of
// other panels, like the tree's expanded folders, stay as they are. Each tab
// may have an editor buffer; a tab with unsaved text is never closed without
// asking. A file opened with a single click is a preview tab that the next
// one replaces; pinned tabs sit first and outlast Close others / Close all.

import { tick, untrack } from "svelte";
import type { ProjectId, ProjectStatePatch } from "../lib/protocol";
import type { Transport } from "../lib/transport";
import type { MainMode, Workspace } from "../shell/workspace.svelte";
import { Buffers } from "./buffers.svelte";
import { drafts } from "./drafts.svelte";
import { highlighter } from "./highlight/highlighter";
import { LanguageClients } from "./lsp/clients";
import { modeMemory } from "./mode-memory";

/** Beyond this many tabs, the leftmost inactive unpinned tab without unsaved text is closed. */
const MAX_OPEN_FILES = 20;

export class OpenFiles {
  /** Tab order; the pinned tabs come first. */
  files = $state<string[]>([]);
  active = $state<string | null>(null);
  /** Pinned tabs, in tab order: shown compact, kept by Close others and Close all. */
  pinned = $state<string[]>([]);
  /**
   * The preview tab, if any: the last file opened without being kept. The next
   * such file replaces it; editing it, a double click or pinning keeps it.
   */
  preview = $state<string | null>(null);
  /** The mode each tab was last shown in, so switching tabs does not carry one tab's mode to the next. */
  readonly modes = new Map<string, MainMode>();
  /** The project's language servers, once the shell wires them up. */
  lsp: LanguageClients | null = null;
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
    const pinned = (state.pinned_files ?? []).filter((path) => state.open_files.includes(path));
    this.pinned = pinned;
    this.files = [...pinned, ...state.open_files.filter((path) => !pinned.includes(path))];
    this.active = state.active_file;
    return state.active_file;
  }

  isPinned(path: string): boolean {
    return this.pinned.includes(path);
  }

  /**
   * Makes `path` (or nothing) the active file. A file without a tab becomes
   * the preview tab, in the place of the old one unless that has unsaved text.
   */
  opened(path: string | null): Promise<void> {
    if (path === this.active) return this.writes;
    this.active = path;
    if (path && !this.files.includes(path)) {
      const files = [...this.files];
      const old = this.preview;
      const at = old && !this.buffers.isDirty(old) ? files.indexOf(old) : -1;
      if (old && at >= 0) {
        files.splice(at, 1, path);
        this.forget(old);
      } else files.push(path);
      this.preview = path;
      for (let i = 0; files.length > MAX_OPEN_FILES && i < files.length; ) {
        const candidate = files[i] as string;
        if (candidate === path || this.isPinned(candidate) || this.buffers.isDirty(candidate)) {
          i++;
        } else {
          files.splice(i, 1);
          this.forget(candidate);
        }
      }
      this.files = files;
    }
    return this.write();
  }

  /** Turns the preview tab `path` into a tab that stays. */
  makePermanent(path: string): void {
    if (this.preview === path) this.preview = null;
  }

  /** Pins `path` after the other pinned tabs, or unpins it to the first unpinned place. */
  togglePin(path: string): void {
    if (!this.files.includes(path)) return;
    this.makePermanent(path);
    const pinned = this.isPinned(path) ? this.pinned.filter((p) => p !== path) : [...this.pinned, path];
    const rest = this.files.filter((p) => !pinned.includes(p) && p !== path);
    this.pinned = pinned;
    this.files = this.isPinned(path) ? [...pinned, ...rest] : [...pinned, path, ...rest];
    void this.write();
  }

  /**
   * Closes every tab except `path`, the pinned ones and those with unsaved
   * text (which would need asking); `null` closes all of them. Returns how
   * many tabs stayed open only because of unsaved text.
   */
  closeOthers(path: string | null): number {
    const unsaved = this.files.filter((p) => p !== path && !this.isPinned(p) && this.buffers.isDirty(p));
    const stays = (p: string) => p === path || this.isPinned(p) || unsaved.includes(p);
    for (const p of this.files) if (!stays(p)) this.forget(p);
    this.files = this.files.filter(stays);
    if (this.preview && !this.files.includes(this.preview)) this.preview = null;
    if (this.active && !this.files.includes(this.active)) this.active = path ?? this.files[0] ?? null;
    void this.write();
    return unsaved.length;
  }

  /**
   * Adds a tab for `path` at the end without showing it, as a rename does for
   * the files it changed. The tab opens in the editor, where those changes wait.
   */
  keep(path: string): void {
    if (this.files.includes(path)) return;
    this.files = [...this.files, path];
    this.modes.set(path, "edit");
    void this.write();
  }

  /** Removes a tab and its buffer; returns the file to show next. */
  close(path: string): string | null {
    const index = this.files.indexOf(path);
    if (index === -1) return this.active;
    this.forget(path);
    this.files = this.files.filter((p) => p !== path);
    this.pinned = this.pinned.filter((p) => p !== path);
    if (this.preview === path) this.preview = null;
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

  private forget(path: string): void {
    this.buffers.discard(path);
    this.modes.delete(path);
  }

  private write(): Promise<void> {
    const state: ProjectStatePatch = {
      open_files: this.files,
      active_file: this.active,
      pinned_files: this.pinned,
    };
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
 * The open project's tabs, or null before its state is loaded, the tab
 * waiting for a Save / Don't save / Cancel answer, and whether its Save is
 * still writing (the question stays until it is done).
 */
export const tabs = $state<{
  current: OpenFiles | null;
  closing: string | null;
  saving: boolean;
  notice: string | null;
}>({
  current: null,
  closing: null,
  saving: false,
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
  if (tabs.saving) return;
  if (answer === "save" && current && path) {
    // The question stays up, with its buttons off, until the file is written.
    tabs.saving = true;
    let saved = false;
    try {
      saved = await current.buffers.open(path).save();
    } finally {
      tabs.saving = false;
      tabs.closing = null;
    }
    if (!saved) {
      // Show why it failed (conflict or error) instead of closing.
      show(workspace, path, "edit");
      return;
    }
    closeTab(workspace, current, path);
    return;
  }
  tabs.closing = null;
  if (!current || !path || answer === "cancel") return;
  closeTab(workspace, current, path);
}

/** Closes all tabs but `path` (or all with `null`) except pinned ones; unsaved ones stay and say so. */
export function closeOtherTabs(workspace: Target, path: string | null): void {
  const current = tabs.current;
  if (!current) return;
  const before = current.active;
  const unsaved = current.closeOthers(path);
  tabs.notice = unsaved > 0 ? `${unsaved === 1 ? "A tab" : `${unsaved} tabs`} with unsaved changes stayed open.` : null;
  if (current.active !== before) show(workspace, current.active);
  void keepFocus(current.files.length === 0);
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

/**
 * The mode `path` was last shown in, as its tab remembers it; a file without
 * a tab opens the way its kind was last switched to, else in `fresh`.
 */
export function modeOf(path: string, fresh: MainMode = "view"): MainMode {
  return tabs.current?.modes.get(path) ?? modeMemory.get(path) ?? fresh;
}

/** Shows the file at `path` (or nothing) in the mode its tab remembers, unless one is given. */
export function show(workspace: Target, path: string | null, mode?: MainMode): void {
  if (path) workspace.openFile(path, mode ?? modeOf(path));
  else workspace.openPath = null;
}

/** Keeps `tabs.current` in step with the open project; returns a function that stops. */
export function persistOpenFiles(transport: Transport, workspace: Workspace): () => void {
  return $effect.root(() => {
    $effect(() => {
      const project = workspace.project?.id;
      const name = workspace.project?.name ?? "";
      tabs.current = null;
      tabs.closing = null;
      if (!project) return;
      const buffers = new Buffers({ transport, project, drafts, highlighter });
      const files = new OpenFiles(transport, project, buffers);
      const lsp = new LanguageClients({
        transport,
        project,
        name,
        open: (path) => workspace.openFile(path, "edit"),
        note: (text) => {
          tabs.notice = text;
        },
        buffers,
        keep: (path) => files.keep(path),
      });
      files.lsp = lsp;
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
        lsp.close();
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

    $effect(() => {
      const path = workspace.openPath;
      const mode = workspace.mode;
      const current = tabs.current;
      if (path && current) untrack(() => current.modes.set(path, mode));
    });

    // Typing in the preview tab keeps it.
    $effect(() => {
      const current = tabs.current;
      const preview = current?.preview;
      if (current && preview && current.buffers.isDirty(preview)) untrack(() => current.makePermanent(preview));
    });
  });
}
