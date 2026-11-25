// Where focus goes in the git panel when what had it goes away: a file's
// row, else its list's Tab stop, else the commit message. Never a hidden
// heading, and nothing while the sidebar shows another view.

/** The git panel, when the sidebar shows it. */
function panel(): HTMLElement | null {
  const root = document.querySelector<HTMLElement>("[data-testid=git]");
  return root && !root.closest("[hidden]") ? root : null;
}

/** Focuses the row of `path` in the staged or unstaged list, or the nearest place; false when the panel is hidden. */
export function focusRow(path: string, staged: boolean): boolean {
  const root = panel();
  const list = root?.querySelector(`[data-testid=git-${staged ? "staged" : "changes"}]`);
  const target =
    list?.querySelector<HTMLElement>(`[data-row][data-path="${CSS.escape(path)}"]`) ??
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
