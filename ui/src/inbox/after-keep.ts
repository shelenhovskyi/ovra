// The words an agent gets when the person asks it to write tests for kept
// changes. Plain text the person could have typed; it decides nothing.

/** At most this many files are named; the agent finds the rest from git. */
const MAX_FILES = 30;

export function testsPrompt(task: string, files: string[]): string {
  const named = files.slice(0, MAX_FILES).map((f) => `- ${f}`);
  if (files.length > MAX_FILES) named.push(`- and ${files.length - MAX_FILES} more`);
  return [
    `Write tests for the changes I just kept from "${task}".`,
    "",
    "Changed files:",
    ...named,
    "",
    "Add or update tests that cover the new behaviour, following the project's own test style, and run them.",
    "Change only tests unless a test shows a real bug; if it does, say so before fixing it.",
    "Finish with what the tests cover and whether they pass.",
  ].join("\n");
}
