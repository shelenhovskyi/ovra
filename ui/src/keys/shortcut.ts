// Parsing, matching and display of shortcut strings such as `Mod+Shift+P`.
// `Mod` is Cmd on Apple keyboards and Ctrl elsewhere.

export type Shortcut = {
  /** Lower-case `KeyboardEvent.key`, e.g. `p`, `enter`, `f1`. */
  key: string;
  ctrl: boolean;
  meta: boolean;
  alt: boolean;
  shift: boolean;
};

/** Whether `Mod` means Cmd. Decided by the keyboard the browser reports, not by the core. */
export function isApple(): boolean {
  return typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);
}

/** Whether the UI runs in a regular browser tab (headless mode) rather than the desktop window. */
export function inBrowserTab(): boolean {
  return typeof window !== "undefined" && !("__TAURI_INTERNALS__" in window);
}

/**
 * Shortcuts a browser keeps for itself (close tab, switch tabs, print), with
 * the stand-ins used in headless mode. The desktop window gets the originals.
 */
const BROWSER_SAFE: Record<string, string> = {
  "Mod+W": "Alt+W",
  "Ctrl+Tab": "Alt+]",
  "Ctrl+Shift+Tab": "Alt+[",
  "Mod+P": "Alt+P",
  "Mod+Shift+A": "Alt+Shift+A",
  "Mod+Shift+R": "Alt+Shift+R",
  "Mod+Shift+G": "Alt+Shift+G",
};

/** The shortcut that is live where the UI runs: `keys` itself, or its browser-safe stand-in. */
export function liveKeys(keys: string, browser: boolean): string {
  return (browser && BROWSER_SAFE[keys]) || keys;
}

/** Parses `Mod+Shift+P`; returns null for a malformed string. */
export function parseShortcut(text: string, apple: boolean): Shortcut | null {
  const parts = text.split("+");
  const key = parts.pop()?.toLowerCase();
  if (!key) return null;
  const shortcut: Shortcut = { key, ctrl: false, meta: false, alt: false, shift: false };
  for (const part of parts) {
    switch (part) {
      case "Mod":
        if (apple) shortcut.meta = true;
        else shortcut.ctrl = true;
        break;
      case "Ctrl":
        shortcut.ctrl = true;
        break;
      case "Alt":
        shortcut.alt = true;
        break;
      case "Shift":
        shortcut.shift = true;
        break;
      default:
        return null;
    }
  }
  return shortcut;
}

const PUNCTUATION: Record<string, string> = {
  BracketLeft: "[",
  BracketRight: "]",
  Comma: ",",
  Period: ".",
  Slash: "/",
  Semicolon: ";",
  Quote: "'",
  Backslash: "\\",
  Minus: "-",
  Equal: "=",
  Backquote: "`",
};

type KeyEventLike = Pick<KeyboardEvent, "key" | "code" | "ctrlKey" | "metaKey" | "altKey" | "shiftKey">;

/**
 * The pressed key, lower-case. Letters and digits fall back to the physical
 * key when the layout produces something else (non-Latin layouts, Alt on macOS),
 * so `Mod+P` works whatever the input language. Punctuation with Shift is
 * matched by its physical key.
 */
function pressedKey(event: KeyEventLike): string {
  // Shift turns punctuation into another character (` into ~); shortcuts name
  // the unshifted key, as in `Ctrl+Shift+``.
  const shifted = event.shiftKey ? PUNCTUATION[event.code] : undefined;
  if (shifted) return shifted;
  const key = event.key.toLowerCase();
  if (key.length === 1 && key >= " " && key <= "~") return key;
  if (key.length !== 1) return key;
  const physical = /^(?:Key([A-Z])|Digit(\d))$/.exec(event.code);
  if (physical) return (physical[1] ?? physical[2] ?? "").toLowerCase();
  return PUNCTUATION[event.code] ?? key;
}

export function matchesShortcut(event: KeyEventLike, shortcut: Shortcut): boolean {
  return (
    pressedKey(event) === shortcut.key &&
    event.ctrlKey === shortcut.ctrl &&
    event.metaKey === shortcut.meta &&
    event.altKey === shortcut.alt &&
    event.shiftKey === shortcut.shift
  );
}

/** Label for menus and the palette: `⇧⌘P` on Apple keyboards, `Ctrl+Shift+P` elsewhere. */
export function formatShortcut(text: string, apple: boolean): string {
  const shortcut = parseShortcut(text, apple);
  if (!shortcut) return text;
  const key = shortcut.key.length === 1 ? shortcut.key.toUpperCase() : (text.split("+").pop() ?? "");
  if (apple) {
    return `${shortcut.ctrl ? "⌃" : ""}${shortcut.alt ? "⌥" : ""}${shortcut.shift ? "⇧" : ""}${shortcut.meta ? "⌘" : ""}${key}`;
  }
  const mods = [shortcut.ctrl && "Ctrl", shortcut.alt && "Alt", shortcut.shift && "Shift"].filter(Boolean);
  return [...mods, key].join("+");
}
