// The project's trust decision as the Agents header shows it: whether one
// waits, and how many warnings it carries. ProjectTrust (the card in the
// task list) writes it and opens itself through `open`; the header's
// "things to check" menu reads it, so the card can stay out of sight until
// someone asks for it.

export const trustWaiting = $state<{
  /** A decision waits: the project's own settings are not trusted, or changed since. */
  waiting: boolean;
  /** They changed since they were trusted. */
  changed: boolean;
  /** Warning lines the card would show. */
  warnings: number;
  /** Shows the card with its warnings, scrolled into view. */
  open: (() => void) | null;
}>({ waiting: false, changed: false, warnings: 0, open: null });
