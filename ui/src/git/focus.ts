// Where focus goes in the git panel when what had it goes away: a file's
// row, else its list's Tab stop, else the commit message. Never a hidden
// heading, and nothing while the sidebar shows another view.

type DiffTarget = { project: string; path: string; staged: boolean };

/** The row a diff was opened from: switching the diff's side does not change where focus goes back. */
let opened: DiffTarget | null = null;

/** Remembers the row that opens `target`'s diff. */
export function diffOpened(target: DiffTarget): void {
  opened = target;
}

/** Focuses the row the diff of `target` was opened from, else `target`'s own row; false when the panel is hidden. */
export function focusDiffRow(target: DiffTarget): boolean {
  const from = opened?.project === target.project && opened.path === target.path ? opened : target;
  return focusRow(from.path, from.staged);
}

/** The git panel, when the sidebar shows it. */
function panel(): HTMLElement | null {
  const root = document.querySelector<HTMLElement>("[data-testid=git]");
  return root && !root.closest("[hidden]") ? root : null;
}

/** The row of `path` in `list`; found by its data, so no selector is built per path. */
export function rowOf(list: ParentNode | null | undefined, path: string): HTMLElement | undefined {
  return [...(list?.querySelectorAll<HTMLElement>("[data-row]") ?? [])].find((row) => row.dataset.path === path);
}

/** Focuses the row of `path` in the staged or unstaged list (else the other one), or the nearest place; false when the panel is hidden. */
export function focusRow(path: string, staged: boolean): boolean {
  const root = panel();
  const list = root?.querySelector(`[data-testid=git-${staged ? "staged" : "changes"}]`);
  const other = root?.querySelector(`[data-testid=git-${staged ? "changes" : "staged"}]`);
  // The file may have moved to the other list meanwhile (staged from its diff).
  const target =
    rowOf(list, path) ??
    rowOf(other, path) ??
    list?.querySelector<HTMLElement>('[data-row][tabindex="0"]') ??
    root?.querySelector<HTMLElement>("textarea");
  target?.focus();
  return !!target;
}

/** Focuses the Tab stop of a list (`staged`, `changes`), else the commit message. */
export function focusList(kind: "staged" | "changes"): void {
  const root = panel();
  (
    root?.querySelector<HTMLElement>(`[data-testid=git-${kind}] [data-row][tabindex="0"]`) ??
    root?.querySelector<HTMLElement>("textarea")
  )?.focus();
}

/** Focuses the commit message, the next step once changes are staged. */
export function focusMessage(): void {
  panel()?.querySelector<HTMLElement>("textarea")?.focus();
}
