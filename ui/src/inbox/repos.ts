// Review items of one task across several repositories
// (`docs/specs/multi-repo.md`): one item per repository, listed together.
import type { ProjectId, ReviewItem } from "../lib/protocol";

/** Whether `item` is one repository's part of a task across several. */
export function acrossRepos(item: ReviewItem): boolean {
  return (item.task_repos?.length ?? 0) > 1;
}

/** The items that are about the same repository as `item`, for comparing its files with other workers'. */
export function sameRepo(item: ReviewItem, items: readonly ReviewItem[]): ReviewItem[] {
  return items.filter((i) => i.project === item.project);
}

/** The repository's name beside a list entry of a task across repositories, else null. */
export function repoTag(item: ReviewItem, name: (id: ProjectId) => string): string | null {
  return acrossRepos(item) ? name(item.project) : null;
}

/** One line on where the item stands in its task across repositories, else null. */
export function repoLine(item: ReviewItem, name: (id: ProjectId) => string): string | null {
  const repos = item.task_repos ?? [];
  if (repos.length < 2) return null;
  const others = repos.filter((r) => r !== item.project).map(name);
  return `The changes in ${name(item.project)}. This task also changed ${others.join(", ")}: each repository has its own item here, and Keep moves on to the next.`;
}
