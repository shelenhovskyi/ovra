// What an agent's changes did to the project's tests and checks, in words
// (`review.warnings`). The core reads the diff, not the agent's summary; a
// test changed on purpose is normal work, so this only points at the lines.
import type { ReviewWarning, ReviewWarningKind } from "../lib/protocol";

const LABEL: Record<ReviewWarningKind, string> = {
  test_deleted: "Deleted a test file",
  test_edited: "Changed an existing test",
  assertion_removed: "Removed an assertion",
  test_skipped: "Skipped a test",
  check_silenced: "Turned off a lint or type check",
  check_config: "Changed how the project is checked",
};

export type CloserRow = {
  key: string;
  label: string;
  path: string;
  /** `file.ts:12`, or the file name for a whole-file warning. */
  place: string;
  /** The line as the agent left or removed it, plain text. */
  text: string | null;
};

export function closerRows(warnings: readonly ReviewWarning[]): CloserRow[] {
  return warnings.map((w, i) => {
    const name = w.path.split("/").pop() ?? w.path;
    const place = w.line === undefined ? name : `${name}:${w.line}${w.old ? " (removed)" : ""}`;
    return {
      key: `${i}:${w.kind}:${w.path}:${w.line ?? ""}`,
      label: LABEL[w.kind],
      path: w.path,
      place,
      text: w.text ?? null,
    };
  });
}

/** The line above the list. */
export function closerIntro(warnings: readonly ReviewWarning[]): string {
  const tests = warnings.some((w) => w.kind !== "check_silenced" && w.kind !== "check_config");
  const checks = warnings.some((w) => w.kind === "check_silenced" || w.kind === "check_config");
  const what = tests && checks ? "tests and checks" : tests ? "tests" : "checks";
  return `The agent changed ${what}. That can be right; make sure they still test what they should.`;
}
