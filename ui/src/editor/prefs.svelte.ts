// The person's editor preferences, kept in the page's storage so they
// survive a reload; they are per user, not per project. Every field is
// reactive and checked on write: an out-of-range value is clamped or
// ignored, so readers never need to validate. Storage errors only mean the
// preferences last until the window closes.
//
// Who reads what: `fontSize` sizes the editor's and the read-only viewer's
// file text; `wordWrap`, `tabSize` and `insertSpaces` shape editing
// (buffers.svelte.ts, indent.ts); `autoSave` drives autosave.svelte.ts;
// `formatOnSave` has a buffer's language server format it before a save;
// `autoCloseBrackets` adds closing brackets and quotes (brackets.ts).
// Settings > Editor changes them.

const KEY = "ovra.editor.prefs";

/** When files save on their own: never, a moment after the last keystroke, or when focus leaves the editor. */
export type AutoSave = "off" | "after_delay" | "on_focus_change";

const AUTO_SAVE: readonly AutoSave[] = ["off", "after_delay", "on_focus_change"];

/** Font size limits and default in CSS pixels; the default is the editor's long-standing 13.5. */
const FONT_SIZE = { min: 10, max: 24, default: 13.5 } as const;

/** Tab size limits and default, in columns. */
const TAB_SIZE = { min: 1, max: 16, default: 4 } as const;

type Store = Pick<Storage, "getItem" | "setItem">;

type Stored = {
  fontSize: number;
  wordWrap: boolean;
  tabSize: number;
  insertSpaces: boolean;
  autoSave: AutoSave;
  formatOnSave: boolean;
  autoCloseBrackets: boolean;
};

const clamp = (value: number, limits: { min: number; max: number }) =>
  Math.min(limits.max, Math.max(limits.min, value));

export class EditorPrefs {
  #fontSize = $state<number>(FONT_SIZE.default);
  #wordWrap = $state(false);
  #tabSize = $state<number>(TAB_SIZE.default);
  #insertSpaces = $state(true);
  #autoSave = $state<AutoSave>("off");
  #formatOnSave = $state(false);
  #autoCloseBrackets = $state(true);

  constructor(private readonly storage: Store | null) {
    try {
      const raw = storage?.getItem(KEY);
      const stored: Partial<Record<keyof Stored, unknown>> = raw ? JSON.parse(raw) : {};
      if (typeof stored.fontSize === "number") this.#fontSize = clamp(stored.fontSize, FONT_SIZE);
      if (typeof stored.wordWrap === "boolean") this.#wordWrap = stored.wordWrap;
      if (typeof stored.tabSize === "number") this.#tabSize = clamp(Math.round(stored.tabSize), TAB_SIZE);
      if (typeof stored.insertSpaces === "boolean") this.#insertSpaces = stored.insertSpaces;
      if (AUTO_SAVE.includes(stored.autoSave as AutoSave)) this.#autoSave = stored.autoSave as AutoSave;
      if (typeof stored.formatOnSave === "boolean") this.#formatOnSave = stored.formatOnSave;
      if (typeof stored.autoCloseBrackets === "boolean") this.#autoCloseBrackets = stored.autoCloseBrackets;
    } catch {
      // Unreadable or unavailable: the defaults.
    }
  }

  /** File text size in CSS pixels, clamped to `FONT_SIZE`. */
  get fontSize(): number {
    return this.#fontSize;
  }
  set fontSize(value: number) {
    if (!Number.isFinite(value)) return;
    this.#fontSize = clamp(value, FONT_SIZE);
    this.save();
  }

  /** Whether the editor wraps lines longer than its width. */
  get wordWrap(): boolean {
    return this.#wordWrap;
  }
  set wordWrap(value: boolean) {
    this.#wordWrap = value;
    this.save();
  }

  /** Columns per tab stop, a whole number within `TAB_SIZE`. */
  get tabSize(): number {
    return this.#tabSize;
  }
  set tabSize(value: number) {
    if (!Number.isFinite(value)) return;
    this.#tabSize = clamp(Math.round(value), TAB_SIZE);
    this.save();
  }

  /** Whether indenting inserts spaces (the default) or tab characters. */
  get insertSpaces(): boolean {
    return this.#insertSpaces;
  }
  set insertSpaces(value: boolean) {
    this.#insertSpaces = value;
    this.save();
  }

  /** When files save on their own; `off` by default. */
  get autoSave(): AutoSave {
    return this.#autoSave;
  }
  set autoSave(value: AutoSave) {
    if (!AUTO_SAVE.includes(value)) return;
    this.#autoSave = value;
    this.save();
  }

  /** Whether saving first has the file's language server format it; off by default. */
  get formatOnSave(): boolean {
    return this.#formatOnSave;
  }
  set formatOnSave(value: boolean) {
    this.#formatOnSave = value;
    this.save();
  }

  /** Whether typing an opening bracket or quote adds its closing one; on by default. */
  get autoCloseBrackets(): boolean {
    return this.#autoCloseBrackets;
  }
  set autoCloseBrackets(value: boolean) {
    this.#autoCloseBrackets = value;
    this.save();
  }

  /** One pixel larger (`1`), smaller (`-1`), or back to the default (`0`). */
  zoom(step: -1 | 0 | 1): void {
    this.fontSize = step === 0 ? FONT_SIZE.default : this.#fontSize + step;
  }

  private save(): void {
    const stored: Stored = {
      fontSize: this.#fontSize,
      wordWrap: this.#wordWrap,
      tabSize: this.#tabSize,
      insertSpaces: this.#insertSpaces,
      autoSave: this.#autoSave,
      formatOnSave: this.#formatOnSave,
      autoCloseBrackets: this.#autoCloseBrackets,
    };
    try {
      this.storage?.setItem(KEY, JSON.stringify(stored));
    } catch {
      // Quota or disabled storage: the preference lasts as long as the page.
    }
  }
}

function localStorageOrNull(): Store | null {
  try {
    return typeof localStorage === "undefined" ? null : localStorage;
  } catch {
    return null;
  }
}

/** The person's editor preferences; read and assign the fields directly. */
export const editorPrefs = new EditorPrefs(localStorageOrNull());
