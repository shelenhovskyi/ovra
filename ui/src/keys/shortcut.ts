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
 * Shortcuts a browser keeps for itself (close tab, switch tabs, print, new
 * window), with the stand-ins used in headless mode. The desktop window gets the originals.
 */
const BROWSER_SAFE: Record<string, string> = {
  "Mod+W": "Alt+W",
  "Ctrl+Tab": "Alt+]",
  "Ctrl+Shift+Tab": "Alt+[",
  "Mod+P": "Alt+P",
  "Mod+Shift+A": "Alt+Shift+A",
  "Mod+Shift+R": "Alt+Shift+R",
  "Ctrl+Shift+G": "Alt+Shift+G",
  "Mod+N": "Alt+N",
  "Mod+Shift+T": "Alt+Shift+T",
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
 * The pressed key, lower-case. Latin letters come from the layout, so `Mod+Z`
 * follows the key labelled Z. Punctuation always comes from the physical key:
 * layouts put other characters there (an apostrophe on the Ukrainian Backquote
 * key, Shift's `~`, Option's quote marks), and shortcuts name the US key.
 * Other characters fall back to the physical letter or digit, so `Mod+P`
 * works whatever the input language.
 */
function pressedKey(event: KeyEventLike): string {
  const key = event.key.toLowerCase();
  if (key.length !== 1) return key;
  if (key >= "a" && key <= "z") return key;
  const punctuation = PUNCTUATION[event.code];
  if (punctuation) return punctuation;
  if (key >= " " && key <= "~") return key;
  const physical = /^(?:Key([A-Z])|Digit(\d))$/.exec(event.code);
  if (physical) return (physical[1] ?? physical[2] ?? "").toLowerCase();
  return key;
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

/**
 * Label for menus and the palette: `⇧⌘P` on Apple keyboards, `Ctrl+Shift+P`
 * elsewhere. A chord such as `Mod+K Mod+I` is formatted press by press; it is
 * shown only, since the widget that owns it (the editor) handles the keys.
 */
export function formatShortcut(text: string, apple: boolean): string {
  return text
    .split(" ")
    .map((press) => formatPress(press, apple))
    .join(" ");
}

function formatPress(text: string, apple: boolean): string {
  const shortcut = parseShortcut(text, apple);
  if (!shortcut) return text;
  const key = shortcut.key.length === 1 ? shortcut.key.toUpperCase() : (text.split("+").pop() ?? "");
  if (apple) {
    return `${shortcut.ctrl ? "⌃" : ""}${shortcut.alt ? "⌥" : ""}${shortcut.shift ? "⇧" : ""}${shortcut.meta ? "⌘" : ""}${key}`;
  }
  const mods = [shortcut.ctrl && "Ctrl", shortcut.alt && "Alt", shortcut.shift && "Shift"].filter(Boolean);
  return [...mods, key].join("+");
}
