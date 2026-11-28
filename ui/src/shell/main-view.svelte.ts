// What the main area shows instead of the open file: the audit log,
// Reviews, Skills and instructions or History (docs/design/patterns.md: logs, lists and setup open as views,
// never as modals). One view at a time; closing it gives focus back to where
// it was, and opening a file closes it. Kept here rather than in the
// workspace state: only the shell and the views care.

export type MainViewName = "audit" | "reviews" | "catalog" | "history";

export const VIEW_TITLES: Record<MainViewName, string> = {
  audit: "Audit log",
  reviews: "Reviews",
  catalog: "Skills and instructions",
  history: "History",
};

export class MainView {
  current = $state<MainViewName | null>(null);
  #returnTo: HTMLElement | null = null;

  /** Shows `name`; the focus it had comes back when the view closes. */
  show(name: MainViewName): void {
    if (this.current === null) {
      const active = typeof document === "undefined" ? null : document.activeElement;
      this.#returnTo = active instanceof HTMLElement && active !== document.body ? active : null;
    }
    this.current = name;
  }

  /** Back to the open file; `refocus` puts focus where it was before the view opened. */
  close(refocus = true): void {
    if (this.current === null) return;
    this.current = null;
    const back = this.#returnTo;
    this.#returnTo = null;
    if (refocus && back?.isConnected) back.focus();
  }

  toggle(name: MainViewName): void {
    if (this.current === name) this.close();
    else this.show(name);
  }
}

export const mainView = new MainView();

/**
 * Puts focus in the file the main area shows, once it does: the editor's
 * text when it edits, else the file's panel. For a file opened while a view
 * covered the files, where the focus it had went away with the view.
 */
export async function focusOpenFile(editing: boolean): Promise<void> {
  let panel: HTMLElement | null = null;
  for (let frame = 0; frame < 30; frame++) {
    await new Promise((resolve) => requestAnimationFrame(resolve));
    panel = document.getElementById("ovra-tabpanel");
    if (!panel || panel.closest("[hidden]")) continue;
    const text = panel.querySelector<HTMLElement>(".cm-content");
    if (text || !editing) {
      (text ?? panel).focus();
      return;
    }
  }
  panel?.focus();
}
