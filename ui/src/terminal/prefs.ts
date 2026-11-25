// Terminal settings kept in the page's storage, so they survive a reload.

const SCREEN_READER = "ovra.terminal.screenReader";

function storage(): Storage | null {
  try {
    return typeof localStorage === "undefined" ? null : localStorage;
  } catch {
    // Storage can be switched off; the setting then lasts until the window closes.
    return null;
  }
}

export function loadScreenReader(store = storage()): boolean {
  try {
    return store?.getItem(SCREEN_READER) === "on";
  } catch {
    return false;
  }
}

export function saveScreenReader(on: boolean, store = storage()): void {
  try {
    if (on) store?.setItem(SCREEN_READER, "on");
    else store?.removeItem(SCREEN_READER);
  } catch {
    // A full or blocked storage only costs the setting after a reload.
  }
}
