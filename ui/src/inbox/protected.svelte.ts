// Protected files (those that can run code or change agent rules) in the
// reviews inbox: each is kept only once the person has looked at its diff, in
// the version of the files the list shows, since the inbox opened. A
// worker's protected files are copied only when the person also ticks them
// in, after each look; the tick covers the version it was given for.

import type { FileChange, ReviewItem } from "../lib/protocol";

export class ProtectedLooks {
  /** The files whose diff was on screen in this sitting, by `seenKey`. */
  #seen = $state<string[]>([]);
  /** The person ticked "Also apply protected files", for the item version `#tickedFor`. */
  #ticked = $state(false);
  #tickedFor = $state<string | null>(null);

  /** A new sitting: seen means seen in this one, not an earlier one. */
  forget(): void {
    this.#seen = [];
    this.untick();
  }

  untick(): void {
    this.#ticked = false;
  }

  /**
   * `file`'s diff, read from the item version `diffFingerprint`, is on
   * screen: it counts as seen only when that is the version the list shows.
   */
  saw(item: ReviewItem, file: FileChange, diffFingerprint: string | null): void {
    if (!file.protected) return;
    if (item.fingerprint && diffFingerprint !== item.fingerprint) return;
    const key = seenKey(item, file);
    if (!this.#seen.includes(key)) this.#seen = [...this.#seen, key];
  }

  /** The item's protected files whose diff the person has not seen in this version. */
  unseen(item: ReviewItem): FileChange[] {
    return item.files.filter((f) => f.protected && !this.#seen.includes(seenKey(item, f)));
  }

  /** A direct item's protected files are already on disk: Keep keeps them only once each was seen. */
  directWaits(item: ReviewItem): boolean {
    return item.mode === "direct" && this.unseen(item).length > 0;
  }

  /** A worker's protected files that Keep could copy: those not waiting for the direct agent. */
  includable(item: ReviewItem): FileChange[] {
    return item.mode === "isolated" ? item.files.filter((f) => f.protected && !f.blocked) : [];
  }

  includableUnseen(item: ReviewItem): FileChange[] {
    const unseen = this.unseen(item);
    return this.includable(item).filter((f) => unseen.includes(f));
  }

  /** Ticks a worker's protected files in, or out; refused while one of them was not seen. */
  tick(item: ReviewItem, on: boolean): boolean {
    if (this.includableUnseen(item).length > 0) return false;
    this.#ticked = on;
    this.#tickedFor = item.fingerprint;
    return true;
  }

  /** Whether the tick holds for `item` as it is now. */
  ticked(item: ReviewItem | null): boolean {
    return (
      this.#ticked && item !== null && item.fingerprint === this.#tickedFor && this.includableUnseen(item).length === 0
    );
  }

  /** Once the files change, or one needs a new look, the tick is gone for good: the person ticks again. */
  dropStaleTick(item: ReviewItem | null): void {
    if (this.#ticked && !this.ticked(item)) this.#ticked = false;
  }

  /** Whether Keep applies the item's protected files: a direct item's once each was seen, a worker's once ticked. */
  included(item: ReviewItem): boolean {
    if (item.mode === "direct") return !this.directWaits(item) && item.files.some((f) => f.protected);
    return this.ticked(item) && this.includable(item).length > 0;
  }

  /** The protected files a replace could copy: whatever is left, including those that waited at Keep. */
  takeGuarded(item: ReviewItem): FileChange[] {
    if (item.state !== "partly_approved") return [];
    return item.files.filter((f) => f.protected && [f.path, f.from].some((p) => p && item.not_copied.includes(p)));
  }

  /** Before a replace, each protected file it would copy must have been seen too. */
  takeUnseen(item: ReviewItem): FileChange[] {
    const unseen = this.unseen(item);
    return this.takeGuarded(item).filter((f) => unseen.includes(f));
  }
}

/**
 * Which version of a file was seen: by the item's fingerprint (a worker's
 * files, a direct item's protected files) and the file's change, so a
 * later change needs a new look.
 */
function seenKey(review: ReviewItem, f: FileChange): string {
  return [review.id, review.fingerprint ?? "", f.path, f.from ?? "", f.kind, f.additions, f.deletions].join("\n");
}
