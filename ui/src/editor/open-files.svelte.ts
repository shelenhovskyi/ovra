// The open files of a project, shown as tabs and kept in the project state
// so the active file comes back when the project is reopened. Writes are
// partial (`open_files`, `active_file` and `pinned_files` only), so fields of
// other panels, like the tree's expanded folders, stay as they are. Each tab
// may have an editor buffer; a tab with unsaved text is never closed without
// asking. A file opened with a single click is a preview tab that the next
// one replaces; pinned tabs sit first and outlast Close others / Close all.
// A split main area has two editor groups (docs/specs/split-editor.md), each
// with its own `OpenFiles`; they share the project's buffers, and the second
// group's tabs are kept in the state's `second_group`.

import { tick, untrack } from "svelte";
import type { EditorGroup, ProjectId, ProjectState, ProjectStatePatch } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";
import type { GroupIndex, MainMode, Workspace } from "../shell/workspace.svelte";
import { movedPath } from "../tree/ops";
import { autoSaver } from "./autosave";
import { Buffers } from "./buffers.svelte";
import { drafts } from "./drafts.svelte";
import { highlighter } from "./highlight/highlighter";
import { LanguageClients } from "./lsp/clients";
import { modeMemory } from "./mode-memory";

/** Beyond this many tabs, the leftmost inactive unpinned tab without unsaved text is closed. */
const MAX_OPEN_FILES = 20;

/** How many closed tabs Reopen closed tab remembers. */
const MAX_CLOSED = 20;

/** Writes of every group, in order, so a closing group never overwrites the one taking its place. */
let writes = Promise.resolve();

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
  /** Which group's fields of the project state this group writes. */
  slot: GroupIndex = 0;
  /** Whether the other group has a tab for `path` (whose buffer then stays when this tab closes). */
  shared: (path: string) => boolean = () => false;
  /** Tabs closed this session, the last one last, for Reopen closed tab. */
  private closed: string[] = [];

  constructor(
    private readonly transport: Transport,
    readonly project: ProjectId,
    readonly buffers: Buffers,
  ) {}

  /** Loads the stored state into the first group; resolves to all of it. */
  async restore(): Promise<ProjectState> {
    const { data: state } = await this.transport.call({
      method: "projects.get_state",
      params: { id: this.project },
    });
    this.apply(state);
    return state;
  }

  /** Takes the tabs of a stored group. */
  apply(group: Pick<EditorGroup, "open_files" | "active_file"> & { pinned_files?: string[] }): void {
    const pinned = (group.pinned_files ?? []).filter((path) => group.open_files.includes(path));
    this.pinned = pinned;
    this.files = [...pinned, ...group.open_files.filter((path) => !pinned.includes(path))];
    this.active = group.active_file;
  }

  /** A new, empty group for `slot` that shares this one's buffers and language servers. */
  sibling(slot: GroupIndex): OpenFiles {
    const files = new OpenFiles(this.transport, this.project, this.buffers);
    files.lsp = this.lsp;
    files.slot = slot;
    return files;
  }

  /** Whether closing the tab for `path` would lose unsaved text. */
  private unsaved(path: string): boolean {
    return this.buffers.isDirty(path) && !this.shared(path);
  }

  isPinned(path: string): boolean {
    return this.pinned.includes(path);
  }

  /**
   * Makes `path` (or nothing) the active file. A file without a tab becomes
   * the preview tab, in the place of the old one unless that has unsaved text.
   */
  opened(path: string | null): Promise<void> {
    if (path === this.active) return writes;
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
    return this.closeSome(
      this.files.filter((p) => p !== path),
      path,
    );
  }

  /** Closes the tabs after `path` the way `closeOthers` does; `path` becomes active if the active one closed. */
  closeToTheRight(path: string): number {
    const index = this.files.indexOf(path);
    return index < 0 ? 0 : this.closeSome(this.files.slice(index + 1), path);
  }

  /** Closes every tab without unsaved text except the pinned ones. */
  closeSaved(): void {
    this.closeSome(
      this.files.filter((p) => !this.buffers.isDirty(p)),
      null,
    );
  }

  /**
   * Closes `candidates` but the pinned ones and those with unsaved text; an
   * active tab that closes hands over to `next`, else the first one left.
   * Returns how many stayed open only because of unsaved text.
   */
  private closeSome(candidates: string[], next: string | null): number {
    const unpinned = candidates.filter((p) => !this.isPinned(p));
    const unsaved = unpinned.filter((p) => this.unsaved(p));
    const closing = unpinned.filter((p) => !unsaved.includes(p));
    const stays = (p: string) => !closing.includes(p);
    for (const p of closing) this.forget(p);
    this.remember(closing);
    this.files = this.files.filter(stays);
    if (this.preview && !stays(this.preview)) this.preview = null;
    if (this.active && !stays(this.active)) this.active = next ?? this.files[0] ?? null;
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
    this.remember([path]);
    this.files = this.files.filter((p) => p !== path);
    this.pinned = this.pinned.filter((p) => p !== path);
    if (this.preview === path) this.preview = null;
    if (this.active === path) this.active = this.files[index] ?? this.files[index - 1] ?? null;
    void this.write();
    return this.active;
  }

  /**
   * `from` (a file, or a folder with everything in it) was renamed or moved
   * to `to`: its tabs follow in place, keeping pin, preview, mode and buffer.
   */
  renamePath(from: string, to: string): void {
    const next = (path: string) => movedPath(path, from, to) ?? path;
    if (this.files.every((path) => next(path) === path)) return;
    for (const path of this.files) {
      const target = next(path);
      if (target === path) continue;
      const buffer = this.buffers.rename(path, target);
      if (buffer) this.lsp?.moved(buffer);
      const mode = this.modes.get(path);
      this.modes.delete(path);
      if (mode) this.modes.set(target, mode);
    }
    this.files = this.files.map(next);
    this.closed = this.closed.map(next);
    this.pinned = this.pinned.map(next);
    if (this.preview) this.preview = next(this.preview);
    if (this.active) this.active = next(this.active);
    void this.write();
  }

  /**
   * `paths` (files or folders) are gone: their tabs close, except those with
   * unsaved text, which stay and show that the file was deleted. Returns the
   * file to show next.
   */
  forgetPaths(paths: string[]): string | null {
    const gone = (path: string) => paths.some((dir) => movedPath(path, dir, dir) !== null);
    this.closed = this.closed.filter((path) => !gone(path));
    const closing = this.files.filter((path) => gone(path) && !this.unsaved(path));
    for (const path of this.files)
      if (gone(path) && !closing.includes(path)) void this.buffers.get(path)?.diskChanged();
    if (closing.length === 0) return this.active;
    const stays = (path: string) => !closing.includes(path);
    if (this.active && !stays(this.active)) {
      const index = this.files.indexOf(this.active);
      this.active = this.files.slice(index + 1).find(stays) ?? this.files.slice(0, index).reverse().find(stays) ?? null;
    }
    for (const path of closing) this.forget(path);
    this.files = this.files.filter(stays);
    this.pinned = this.pinned.filter(stays);
    if (this.preview && !stays(this.preview)) this.preview = null;
    void this.write();
    return this.active;
  }

  /**
   * Takes the most recently closed tab back, skipping files that no longer
   * exist; null when none is left. The caller shows it.
   */
  async reopen(): Promise<string | null> {
    for (let path = this.closed.pop(); path !== undefined; path = this.closed.pop()) {
      if (this.files.includes(path)) continue;
      try {
        await this.transport.call({ method: "fs.read_file", params: { project: this.project, path } });
      } catch (err) {
        // Gone files are skipped; any other error is the viewer's to show.
        if (err instanceof CoreError && err.code === "not_found") continue;
      }
      await this.opened(path);
      this.makePermanent(path);
      return path;
    }
    return null;
  }

  /** The tab `offset` places away from the active one, wrapping around. */
  neighbour(offset: number): string | null {
    if (this.files.length === 0) return null;
    const index = this.active ? this.files.indexOf(this.active) : -1;
    const count = this.files.length;
    return this.files[(((index + offset) % count) + count) % count] ?? null;
  }

  private remember(paths: string[]): void {
    this.closed = [...this.closed.filter((p) => !paths.includes(p)), ...paths].slice(-MAX_CLOSED);
  }

  private forget(path: string): void {
    if (!this.shared(path)) this.buffers.discard(path);
    this.modes.delete(path);
  }

  /** This group is the only one left: it writes the first group's fields and clears the second's. */
  becomeOnly(): void {
    this.slot = 0;
    this.shared = () => false;
    void this.write({ second_group: { open_files: [], active_file: null, pinned_files: [] } });
  }

  private write(also: ProjectStatePatch = {}): Promise<void> {
    const group: EditorGroup = { open_files: this.files, active_file: this.active, pinned_files: this.pinned };
    const state: ProjectStatePatch = { ...(this.slot === 0 ? group : { second_group: group }), ...also };
    writes = writes
      .then(async () => {
        await this.transport.call({ method: "projects.set_state", params: { id: this.project, state } });
      })
      .catch(() => {
        // Losing the list of open files is harmless; the next change writes it again.
      });
    return writes;
  }
}

/**
 * The open project's editor groups, left to right (none before its state is
 * loaded), the tab waiting for a Save / Don't save / Cancel answer, and
 * whether its Save is still writing (the question stays until it is done).
 */
class Tabs {
  groups = $state<OpenFiles[]>([]);
  closing = $state<string | null>(null);
  saving = $state(false);
  notice = $state<string | null>(null);
  /** The focused group, as the workspace says; `persistOpenFiles` connects it. */
  focused: () => GroupIndex = () => 0;

  /** The focused group's tabs, which commands act on. */
  get current(): OpenFiles | null {
    return this.groups[this.focused()] ?? null;
  }

  /** One group, or none. */
  set current(files: OpenFiles | null) {
    this.groups = files ? [files] : [];
  }
}

export const tabs = new Tabs();

type Target = Pick<
  Workspace,
  "openPath" | "mode" | "openFile" | "other" | "group" | "focusGroup" | "split" | "closeGroup"
>;

/** The tab being dragged, so the other group can take it (an HTML drag does not say what it carries until the drop). */
export const dragging = $state<{ tab: { group: GroupIndex; path: string } | null }>({ tab: null });

/** Connects a new group to the others, so a tab both have keeps its buffer when one closes it. */
function joined(files: OpenFiles): OpenFiles {
  files.shared = (path) => tabs.groups.some((group) => group !== files && group.files.includes(path));
  return files;
}

/** The mode `path` was last shown in within `group`. */
function modeIn(group: OpenFiles, path: string): MainMode {
  return group.modes.get(path) ?? modeMemory.get(path) ?? "view";
}

/**
 * Split right: the focused group's file opens in a new group on the right,
 * which takes focus; with two groups, focus moves to the other one.
 */
export function splitRight(workspace: Target): void {
  const current = tabs.current;
  if (workspace.other) {
    workspace.focusGroup(workspace.group === 0 ? 1 : 0);
    void focusMain();
    return;
  }
  const path = workspace.openPath;
  if (!current || !path) return;
  const right = joined(current.sibling(1));
  joined(current);
  tabs.groups = [current, right];
  right.modes.set(path, workspace.mode);
  void right.opened(path);
  right.makePermanent(path);
  workspace.split({ openPath: path, mode: workspace.mode });
  void focusMain();
}

/** Focuses group `group`, if there is one, and moves focus into it. */
export function focusGroup(workspace: Target, group: GroupIndex): void {
  if (!tabs.groups[group]) return;
  workspace.focusGroup(group);
  void focusMain();
}

/**
 * Moves the tab for `path` from the group it is in to the other one, which
 * shows it and takes focus; with one group, a new group on the right takes
 * it, unless it is the only tab. The group it leaves closes if it was its last.
 */
export function moveTab(workspace: Target, path: string, to: GroupIndex): void {
  const fromIndex: GroupIndex = to === 0 ? 1 : 0;
  const from = tabs.groups[fromIndex];
  if (!from?.files.includes(path)) return;
  const mode = modeIn(from, path);
  let target = tabs.groups[to];
  if (!target) {
    if (to === 0 || from.files.length < 2) return;
    target = joined(from.sibling(1));
    joined(from);
    tabs.groups = [from, target];
  }
  target.modes.set(path, mode);
  void target.opened(path);
  target.makePermanent(path);
  // The other group shows the moved file, so closing this tab keeps its buffer.
  workspace.focusGroup(fromIndex);
  const next = from.close(path);
  show(workspace, next);
  if (workspace.other) {
    workspace.focusGroup(to);
    workspace.openFile(path, mode);
  } else workspace.split({ openPath: path, mode });
  closeEmptyGroup(workspace);
  void focusMain();
}

/** With two groups, one without tabs closes; the other takes the whole width. */
function closeEmptyGroup(workspace: Target): void {
  if (tabs.groups.length < 2) return;
  const empty = tabs.groups.findIndex((group) => group.files.length === 0);
  if (empty < 0) return;
  const left = tabs.groups.filter((_, index) => index !== empty);
  tabs.groups = left;
  workspace.closeGroup(empty === 0 ? 0 : 1);
  left[0]?.becomeOnly();
}

/** Unsaved editor text for a file of the open project, or null. Reactive; for the viewer. */
export function unsavedText(project: ProjectId, path: string): string | null {
  const current = tabs.current;
  return current?.project === project ? current.buffers.unsavedText(path) : null;
}

/** Closes a tab, first asking (via `tabs.closing`) if it has unsaved text. */
export function requestClose(workspace: Target, path: string): void {
  const current = tabs.current;
  if (!current) return;
  if (current.buffers.isDirty(path) && !current.shared(path)) {
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

/** Shows the most recently closed tab again, if one of them still exists. */
export async function reopenClosedTab(workspace: Target): Promise<void> {
  const path = await tabs.current?.reopen();
  if (path) show(workspace, path);
}

/**
 * Saves every file with unsaved text, loaded or only a draft. Files that
 * could not be saved (changed on disk, or an error) keep their text, show
 * why in their editor as Save does, and are named in a notice.
 */
export async function saveAll(): Promise<void> {
  const current = tabs.current;
  if (!current) return;
  const { buffers } = current;
  const open = tabs.groups.flatMap((group) => group.files);
  const paths = [...new Set([...open.filter((p) => buffers.isDirty(p)), ...buffers.unsavedPaths()])];
  const saveOne = async (path: string) => {
    const buffer = buffers.open(path);
    await buffer.loaded;
    // A draft made against another version is a conflict the person settles, never overwritten here.
    if (buffer.conflict) return false;
    return !buffer.dirty || buffer.save();
  };
  const saved = await Promise.all(paths.map(saveOne));
  const failed = paths.filter((_, i) => !saved[i]);
  tabs.notice = failed.length > 0 ? `Not saved, open to see why: ${failed.join(", ")}.` : null;
}

/** Closes all tabs but `path` (or all with `null`) except pinned ones; unsaved ones stay and say so. */
export function closeOtherTabs(workspace: Target, path: string | null): void {
  closeTabsBy(workspace, (current) => current.closeOthers(path));
}

/** Closes the tabs to the right of `path` except pinned ones; unsaved ones stay and say so. */
export function closeTabsToTheRight(workspace: Target, path: string): void {
  closeTabsBy(workspace, (current) => current.closeToTheRight(path));
}

/** Closes the tabs without unsaved text, except pinned ones. */
export function closeSavedTabs(workspace: Target): void {
  closeTabsBy(workspace, (current) => {
    current.closeSaved();
    return 0;
  });
}

function closeTabsBy(workspace: Target, close: (current: OpenFiles) => number): void {
  const current = tabs.current;
  if (!current) return;
  const before = current.active;
  const unsaved = close(current);
  tabs.notice = unsaved > 0 ? `${unsaved === 1 ? "A tab" : `${unsaved} tabs`} with unsaved changes stayed open.` : null;
  if (current.active !== before) show(workspace, current.active);
  closed(workspace, current);
}

/** `from` was renamed or moved to `to`: the tabs of both groups follow, and they keep showing the file. */
export function renameTabs(workspace: Target, from: string, to: string): void {
  for (const group of tabs.groups) group.renamePath(from, to);
  const shown = workspace.openPath === null ? null : movedPath(workspace.openPath, from, to);
  if (shown) workspace.openPath = shown;
  const other = workspace.other;
  const moved = other?.openPath ? movedPath(other.openPath, from, to) : null;
  if (other && moved) workspace.other = { ...other, openPath: moved };
}

/** `paths` went to the Trash: their tabs close in both groups, except unsaved ones, which say the file was deleted. */
export function forgetTabs(workspace: Target, paths: string[]): void {
  const current = tabs.current;
  if (!current) return;
  const other = tabs.groups.find((group) => group !== current);
  const before = other?.active;
  const next = other?.forgetPaths(paths);
  if (other && workspace.other && next !== before)
    workspace.other = { openPath: next ?? null, mode: next ? modeIn(other, next) : "view" };
  const was = current.active;
  const shown = current.forgetPaths(paths);
  if (shown !== was) show(workspace, shown);
  if (shown !== was || (other && next !== before)) closed(workspace, current);
}

function closeTab(workspace: Target, current: OpenFiles, path: string): void {
  const wasActive = current.active === path;
  const next = current.close(path);
  if (wasActive) show(workspace, next);
  closed(workspace, current);
}

/** After tabs closed in `current`: an emptied group closes, and focus stays in the window. */
function closed(workspace: Target, current: OpenFiles): void {
  const last = current.files.length === 0;
  closeEmptyGroup(workspace);
  void keepFocus(last && tabs.groups.length === 1 && tabs.groups[0]?.files.length === 0);
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
  // With two editor groups, the focused one's.
  const group = panel?.querySelector<HTMLElement>("[data-focused]");
  const scope = group && group.offsetParent !== null ? group : panel;
  const shown = (selector: string) =>
    [...(scope?.querySelectorAll<HTMLElement>(selector) ?? [])].find((element) => element.offsetParent !== null);
  // The editor's text before its scroller, which is focusable too.
  const target = shown(".cm-content") ?? shown("[data-panel] [tabindex='-1']") ?? panel;
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
  tabs.focused = () => workspace.group;
  return $effect.root(() => {
    $effect(() => {
      const project = workspace.project?.id;
      const name = workspace.project?.name ?? "";
      tabs.current = null;
      tabs.closing = null;
      // Another project opens with one group, until its own state says otherwise.
      untrack(() => {
        workspace.other = null;
        workspace.group = 0;
      });
      if (!project) return;
      const buffers = new Buffers({ transport, project, drafts, highlighter, autoSave: autoSaver });
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
        keep: (path) => (tabs.current ?? files).keep(path),
      });
      files.lsp = lsp;
      let live = true;
      let groups = [files];
      const ready = () => {
        if (!live) return;
        tabs.groups = groups;
        void files.opened(workspace.openPath);
      };
      files.restore().then((state) => {
        if (!live) return;
        const active = state.active_file;
        if (active && workspace.openPath === null) workspace.openFile(active);
        // A split main area comes back with the left group focused.
        const second = state.second_group;
        if (second && second.open_files.length > 0) {
          const right = joined(files.sibling(1));
          joined(files);
          right.apply(second);
          right.active = right.active && right.files.includes(right.active) ? right.active : (right.files[0] ?? null);
          groups = [files, right];
          workspace.other = { openPath: right.active, mode: right.active ? modeIn(right, right.active) : "view" };
        }
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
