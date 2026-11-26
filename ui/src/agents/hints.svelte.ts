// What the person has already done or been told once, remembered in this
// browser: one-time hint lines and the three first steps go away for good.

export type Hint =
  /** Answered a first approval. */
  | "answered"
  /** Started a first task. */
  | "started"
  /** Opened a first task that ended with nothing to review. */
  | "read"
  /** Opened a first review. */
  | "reviewed"
  /** Saw the "Done · Changed …" line. */
  | "done_line"
  /** Closed the first steps. */
  | "steps_hidden";

const KEY = "ovra.hints";

function read(): Partial<Record<Hint, true>> {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(KEY) ?? "{}");
    return typeof value === "object" && value !== null ? (value as Partial<Record<Hint, true>>) : {};
  } catch {
    return {};
  }
}

const seen = $state(read());

export const hints = {
  has(hint: Hint): boolean {
    return seen[hint] === true;
  },
  mark(hint: Hint): void {
    if (seen[hint]) return;
    seen[hint] = true;
    try {
      localStorage.setItem(KEY, JSON.stringify(seen));
    } catch {
      // Storage may be full or blocked: remembered for this window only.
    }
  },
};
