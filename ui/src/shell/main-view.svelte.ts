// What the main area shows instead of the open file: Settings, the audit log
// or Reviews (docs/design/patterns.md: logs, lists and setup open as views,
// never as modals). One view at a time; closing it gives focus back to where
// it was, and opening a file closes it. Kept here rather than in the
// workspace state: only the shell and the views care.

export type MainViewName = "settings" | "audit" | "reviews";

export const VIEW_TITLES: Record<MainViewName, string> = {
  settings: "Settings",
  audit: "Audit log",
  reviews: "Reviews",
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
