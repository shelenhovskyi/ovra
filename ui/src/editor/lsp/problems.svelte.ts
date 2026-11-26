// Every diagnostic the language servers published, per project, also for
// files the editor does not show: what the Problems list and the status
// bar's counts read. A server's `publishDiagnostics` replaces what it said
// about that file before; a server that stops or is dropped takes its
// problems with it. Hints are left out, as the editor's gutter shows them.

import type { ProjectId, ServerId } from "../../lib/protocol";
import type { TextSpot } from "../../shell/workspace.svelte";

export type Severity = "error" | "warning" | "info";

export type Problem = {
  severity: Severity;
  /** The message on one line. */
  message: string;
  /** Who said it ("ts", "clippy"), if the server says. */
  source: string | null;
  /** Where it starts, both from 1, for showing. */
  line: number;
  column: number;
  /** What to select when it opens. */
  spot: TextSpot;
};

export type FileProblems = { path: string; problems: Problem[] };

export type ProblemCounts = Record<Severity, number>;

const SEVERITIES: Record<number, Severity> = { 1: "error", 2: "warning", 3: "info" };
const RANK: Record<Severity, number> = { error: 0, warning: 1, info: 2 };

type Position = { line: number; character: number };

function position(value: unknown): Position | null {
  const p = value as Partial<Position> | null;
  return p && Number.isInteger(p.line) && Number.isInteger(p.character) && (p.line ?? -1) >= 0
    ? { line: p.line as number, character: Math.max(0, p.character as number) }
    : null;
}

/** The problems in one `publishDiagnostics` list, most severe first, then by place; malformed entries are dropped. */
export function toProblems(diagnostics: unknown): Problem[] {
  if (!Array.isArray(diagnostics)) return [];
  const problems: Problem[] = [];
  for (const raw of diagnostics as {
    range?: { start?: unknown; end?: unknown };
    severity?: unknown;
    message?: unknown;
    source?: unknown;
  }[]) {
    const start = position(raw?.range?.start);
    const end = position(raw?.range?.end);
    // A missing severity is the client's to choose; the editor shows it as an error.
    const severity = raw?.severity === undefined ? "error" : SEVERITIES[raw.severity as number];
    if (!start || !severity || typeof raw.message !== "string") continue;
    const to = end && end.line === start.line ? Math.max(end.character, start.character) : start.character;
    problems.push({
      severity,
      message: raw.message.replace(/\s+/g, " ").trim(),
      source: typeof raw.source === "string" ? raw.source : null,
      line: start.line + 1,
      column: start.character + 1,
      spot: { line: start.line + 1, from: start.character, to },
    });
  }
  return problems.sort((a, b) => RANK[a.severity] - RANK[b.severity] || a.line - b.line || a.column - b.column);
}

/** Problems by server, then file, grouped by file: files with errors first, then warnings, then info; then by path. */
export function groupByFile(byServer: Record<ServerId, Record<string, Problem[]>>): FileProblems[] {
  const files = new Map<string, Problem[]>();
  for (const byPath of Object.values(byServer)) {
    for (const [path, problems] of Object.entries(byPath)) files.set(path, [...(files.get(path) ?? []), ...problems]);
  }
  const worst = (problems: Problem[]) => Math.min(...problems.map((p) => RANK[p.severity]));
  return [...files]
    .filter(([, problems]) => problems.length > 0)
    .map(([path, problems]) => ({
      path,
      problems: problems.sort((a, b) => RANK[a.severity] - RANK[b.severity] || a.line - b.line || a.column - b.column),
    }))
    .sort((a, b) => worst(a.problems) - worst(b.problems) || (a.path < b.path ? -1 : a.path > b.path ? 1 : 0));
}

/** How many problems of each severity `files` hold. */
export function countProblems(files: FileProblems[]): ProblemCounts {
  const counts: ProblemCounts = { error: 0, warning: 0, info: 0 };
  for (const file of files) for (const problem of file.problems) counts[problem.severity]++;
  return counts;
}

/** The status bar's words for `counts`, such as "2 errors, 1 warning"; empty while there are none of either. */
export function countsLabel(counts: ProblemCounts): string {
  const part = (n: number, one: string) => (n === 0 ? "" : `${n} ${one}${n === 1 ? "" : "s"}`);
  return [part(counts.error, "error"), part(counts.warning, "warning")].filter(Boolean).join(", ");
}

class ProblemStore {
  /** By project, then server, then path. */
  private published = $state.raw<Record<ProjectId, Record<ServerId, Record<string, Problem[]>>>>({});

  /** Server `server` said `diagnostics` about `path`: they replace what it said before. */
  publish(project: ProjectId, server: ServerId, path: string, diagnostics: unknown): void {
    const servers = this.published[project] ?? {};
    const files = { ...servers[server] };
    const problems = toProblems(diagnostics);
    if (problems.length > 0) files[path] = problems;
    else delete files[path];
    this.published = { ...this.published, [project]: { ...servers, [server]: files } };
  }

  /** Server `server` is gone: its problems go too. */
  forget(project: ProjectId, server: ServerId): void {
    const servers = this.published[project];
    if (!servers?.[server]) return;
    const { [server]: _gone, ...rest } = servers;
    this.published = { ...this.published, [project]: rest };
  }

  /** The project's problems, grouped by file. Reactive. */
  files(project: ProjectId | null | undefined): FileProblems[] {
    return project ? groupByFile(this.published[project] ?? {}) : [];
  }
}

/** The problems of every project; the language clients fill it. */
export const problems = new ProblemStore();

/** Whether the Problems tab of the bottom dock is shown. */
export const problemsDock = $state({ shown: false });
