// The @ menu's GitHub rows (docs/specs/task-context.md, "GitHub issues and
// failed checks"): typing `@#` lists the failed checks of the branch's pull
// request and the repository's open issues, read through the person's own
// `gh` only then. Choosing one reads its text or log and adds it as a chip.

import type { BranchPr, ContextWhat, IssueSummary, ProjectId } from "../lib/protocol";
import type { Transport } from "../lib/transport";
import { rank } from "../palette/fuzzy";

/** What `@#` lists: the open issues and the branch's pull request, if any. */
export type GitHubList = { issues: IssueSummary[]; pr: BranchPr | null };

/** A GitHub row: its chip's key, its name and what it reads when chosen. */
export type GitHubRow = {
  key: string;
  label: string;
  detail: string;
  icon: "issue" | "error";
  load: () => Promise<ContextWhat>;
};

/** Reads the open issues and the branch's failed checks; the checks are left out when reading them fails. */
export async function loadGitHub(transport: Transport, project: ProjectId): Promise<GitHubList> {
  const checks = transport.call({ method: "pr.branch_checks", params: { project } }).then(
    (answer) => answer.data,
    () => null,
  );
  const issues = await transport.call({ method: "pr.issues", params: { project } });
  return { issues: issues.data, pr: await checks };
}

/** The key of a chip's item, as `githubRows` keys its rows; null for other kinds. */
export function githubKey(what: ContextWhat): string | null {
  if (what.kind === "issue") return `issue ${what.number}`;
  if (what.kind === "check") return `check ${what.pr} ${what.name}`;
  return null;
}

/**
 * The rows for `query` (what follows `@#`): failed checks first, then issues.
 * A number that no listed issue has still gets a row, for an older issue.
 */
export function githubRows(
  transport: Transport,
  project: ProjectId,
  list: GitHubList,
  query: string,
  max: number,
): GitHubRow[] {
  const pr = list.pr;
  const checks: GitHubRow[] = (pr?.failed ?? []).flatMap((check) => {
    const run = check.run;
    if (run === null) return [];
    return [
      {
        key: `check ${pr?.number} ${check.name}`,
        label: check.name,
        detail: `Failed check of pull request #${pr?.number}`,
        icon: "error",
        load: async () => {
          const log = (await transport.call({ method: "pr.check_log", params: { project, run } })).data;
          return { kind: "check", pr: log.pr, name: log.name, url: log.url, text: log.text };
        },
      },
    ];
  });
  const issueRow = (number: number, title: string): GitHubRow => ({
    key: `issue ${number}`,
    label: `#${number}`,
    detail: title,
    icon: "issue",
    load: async () => {
      const issue = (await transport.call({ method: "pr.issue", params: { project, number } })).data;
      return { kind: "issue", ...issue };
    },
  });
  const typed = query.trim();
  if (!typed) return [...checks, ...list.issues.map((i) => issueRow(i.number, i.title))].slice(0, max);
  if (/^\d{1,9}$/.test(typed)) {
    const issues = list.issues.filter((i) => String(i.number).startsWith(typed));
    const exact = issues.some((i) => String(i.number) === typed);
    const rows = issues.map((i) => issueRow(i.number, i.title));
    return (exact ? rows : [issueRow(Number(typed), `Read issue #${typed}`), ...rows]).slice(0, max);
  }
  const rows = [...checks, ...list.issues.map((i) => issueRow(i.number, i.title))];
  return rank(rows, (row) => `${row.label} ${row.detail}`, typed, max).map((r) => r.item);
}

/** Words that offer the GitHub rows' entry in the plain @ menu. */
const ENTRY_WORDS = ["github", "issue", "issues", "check", "checks", "ci"];

/** Whether the plain @ menu offers "GitHub issue or failed check" for `query`: always when nothing is typed. */
export function offersGitHub(query: string): boolean {
  const typed = query.toLowerCase();
  return typed === "" || (typed.length >= 2 && ENTRY_WORDS.some((word) => word.startsWith(typed)));
}
