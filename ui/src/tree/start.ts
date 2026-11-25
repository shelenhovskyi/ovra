// Words for the start screen's recent projects: a short path and when each
// was last opened.

/** `root` with the home folder as `~`, guessed from the usual places (the UI does not know the OS). */
export function shortPath(root: string): string {
  const home = /^(\/home\/[^/]+|\/Users\/[^/]+|\/root|[A-Za-z]:\\Users\\[^\\]+)(?=$|[/\\])/.exec(root);
  return home ? `~${root.slice(home[0].length)}` : root;
}

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/** "Just now", "5 min ago", "2h ago", "Yesterday", "3 days ago", then the date; empty when never opened. */
export function openedAgo(ms: number | null, now: number): string {
  if (ms === null) return "";
  const ago = Math.max(0, now - ms);
  if (ago < MINUTE) return "Just now";
  if (ago < HOUR) return `${Math.floor(ago / MINUTE)} min ago`;
  if (ago < DAY) return `${Math.floor(ago / HOUR)}h ago`;
  if (ago < 2 * DAY) return "Yesterday";
  if (ago < 7 * DAY) return `${Math.floor(ago / DAY)} days ago`;
  return new Date(ms).toLocaleDateString("en", { month: "short", day: "numeric" });
}
