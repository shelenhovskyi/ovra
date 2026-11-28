// Whether the Settings window is open (owner, 2026-11-28: a window over the
// app, like macOS System Settings, not a view in the main area). Closing it
// gives focus back to where it was.

class SettingsWindow {
  open = $state(false);
  #returnTo: HTMLElement | null = null;

  show(): void {
    if (this.open) return;
    const active = typeof document === "undefined" ? null : document.activeElement;
    this.#returnTo = active instanceof HTMLElement && active !== document.body ? active : null;
    this.open = true;
  }

  /** `refocus` puts focus where it was before the window opened. */
  close(refocus = true): void {
    if (!this.open) return;
    this.open = false;
    const back = this.#returnTo;
    this.#returnTo = null;
    if (refocus && back?.isConnected) back.focus();
  }

  toggle(): void {
    if (this.open) this.close();
    else this.show();
  }
}

export const settingsWindow = new SettingsWindow();
