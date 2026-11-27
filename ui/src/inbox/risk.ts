// The order Reviews lists a review's files in: the ones most worth a careful
// look first. A plain guess from each file's path and change; it hides
// nothing and decides nothing.

import type { FileChange } from "../lib/protocol";

/** Files that decide what gets installed, built or run, and CI. */
const BUILD =
  /(^|\/)(package\.json|pnpm-lock\.yaml|package-lock\.json|yarn\.lock|Cargo\.toml|Cargo\.lock|go\.mod|go\.sum|pyproject\.toml|requirements[^/]*\.txt|Gemfile(\.lock)?|Dockerfile|docker-compose[^/]*\.ya?ml|Makefile|justfile|build\.rs|deny\.toml)$|^\.github\//i;
/** Names that usually mean access, secrets or money. */
const SENSITIVE =
  /(^|[/._-])(auth|login|session|secret|password|token|credential|crypto|permission|sandbox|payment|billing|\.env)(?=$|[/._-])/i;
const TESTS = /(^|\/)(tests?|__tests__|spec|e2e)\/|[._-](test|spec)\.[a-z]+$|_test\.(go|rs|py)$/i;
const DOCS = /\.(md|mdx|txt|rst)$/i;
/** Lines added and removed from which a change counts as large. */
const LARGE = 200;

/** Why a file is listed early, in a few words; null for an ordinary one. */
export function riskReason(file: FileChange): string | null {
  // Protected files carry their own marker and reason already.
  if (file.protected) return null;
  if (file.kind === "deleted") return "Deletes a file";
  if (BUILD.test(file.path)) return "Dependencies, build or CI";
  if (SENSITIVE.test(file.path)) return "May touch access or secrets";
  return null;
}

function score(file: FileChange): number {
  if (file.protected) return 4;
  if (riskReason(file)) return 3;
  if (file.additions + file.deletions >= LARGE || file.kind === "renamed" || file.by === "both") return 2;
  if (TESTS.test(file.path) || DOCS.test(file.path)) return 0;
  return 1;
}

/** `files` riskiest first; files alike keep the order they came in. */
export function byRisk(files: readonly FileChange[]): FileChange[] {
  return files
    .map((file, index) => ({ file, index, score: score(file) }))
    .sort((a, b) => b.score - a.score || a.index - b.index)
    .map((f) => f.file);
}
