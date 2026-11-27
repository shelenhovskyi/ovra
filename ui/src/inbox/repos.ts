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
  return `The changes in ${name(item.project)}. This task also changed ${others.join(", ")}: each repository has its own item here. Keep moves on to the next, and Keep all keeps every one.`;
}

/** The open items of `item`'s task across repositories, in the task's order, `item`'s among them. */
export function taskParts(item: ReviewItem, items: readonly ReviewItem[]): ReviewItem[] {
  if (!acrossRepos(item)) return [];
  const order = item.task_repos ?? [];
  return items
    .filter((i) => i.session === item.session && (i.state === "open" || i.state === "published"))
    .sort((a, b) => order.indexOf(a.project) - order.indexOf(b.project));
}

/**
 * Why Keep all waits for these parts, else null. Each part is still kept on
 * its own, with its own fingerprint and check: a running or failed check,
 * or protected files, need that part's own decision.
 */
export function keepAllWaits(parts: readonly ReviewItem[], name: (id: ProjectId) => string): string | null {
  const named = (pick: (i: ReviewItem) => boolean) =>
    parts
      .filter(pick)
      .map((i) => name(i.project))
      .join(", ");
  const running = named((i) => i.check?.state === "running");
  if (running) return `Keep all waits for the check in ${running}.`;
  const failed = named((i) => i.check?.state === "failed");
  if (failed) return `The check failed in ${failed}: keep each repository on its own.`;
  const guarded = named((i) => i.files.some((f) => f.protected));
  if (guarded) return `${guarded} has protected files: keep each repository on its own.`;
  return null;
}
