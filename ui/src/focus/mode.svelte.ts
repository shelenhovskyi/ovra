// Whether the window shows Focus (docs/specs/focus-mode.md): one quiet screen
// about the agents' work instead of the IDE. Kept here rather than in the
// workspace state, like the main view: only the shell and Focus care.

const KEY = "ovra.focus";

export class FocusMode {
  on = $state(read());

  set(on: boolean): void {
    this.on = on;
    try {
      if (on) localStorage.setItem(KEY, "1");
      else localStorage.removeItem(KEY);
    } catch {
      // Storage may be blocked: Focus lasts for this window only.
    }
  }

  toggle(): void {
    this.set(!this.on);
  }
}

function read(): boolean {
  try {
    return localStorage.getItem(KEY) === "1";
  } catch {
    return false;
  }
}

export const focusMode = new FocusMode();

/** A command that shows an IDE panel Focus hides (Git, Search, Files) leaves Focus first, rather than doing nothing. */
export function leaveFocus(): void {
  if (focusMode.on) focusMode.set(false);
}
