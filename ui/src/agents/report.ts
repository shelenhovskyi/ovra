// A research session's answer (ADR 0013): the report the person can save
// under docs/research/ or hand to a task. The UI writes the file through
// `fs.*`, never the agent.

import type { AgentMessage } from "../lib/protocol";

/** Where saved reports go, relative to the project root. */
export const REPORT_DIR = "docs/research";

/** The assistant's text since the last message it was sent, or null while there is none. */
export function reportOf(messages: readonly AgentMessage[] | undefined): string | null {
  if (!messages) return null;
  const parts: string[] = [];
  for (let n = messages.length - 1; n >= 0; n--) {
    const m = messages[n];
    if (!m || m.kind === "user_text") break;
    // A subagent's text is not the answer.
    if (m.kind === "assistant_text" && !m.parent && m.text.trim()) parts.unshift(m.text.trim());
  }
  return parts.length > 0 ? parts.join("\n\n") : null;
}

/** `docs/research/<date>-<title>.md`, with `-2`, `-3`... for the `n`-th try once the name is taken. */
export function reportPath(title: string, date: Date, n = 1): string {
  const day = date.toISOString().slice(0, 10);
  const slug =
    title
      .toLowerCase()
      .normalize("NFKD")
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-+|-+$/g, "")
      .slice(0, 48)
      .replace(/-+$/, "") || "report";
  return `${REPORT_DIR}/${day}-${slug}${n > 1 ? `-${n}` : ""}.md`;
}

/** The saved file: the question as its heading, then the report. */
export function reportFile(question: string, report: string): string {
  return `# ${question.split("\n")[0]?.trim() || "Research"}\n\n${report}\n`;
}

/** The task form's text for "Turn into task": the findings quoted, the person adds what to do. */
export function taskPrompt(question: string, report: string): string {
  const quoted = report
    .split("\n")
    .map((line) => (line ? `> ${line}` : ">"))
    .join("\n");
  return `Research on "${question.split("\n")[0]?.trim()}" found:\n\n${quoted}\n\nTask: `;
}
