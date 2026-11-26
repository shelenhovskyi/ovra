// An agent's answer that lists numbered items ("1. Fix the race in...",
// "2. ..."): the person picks some and each picked item becomes a task for
// its own agent in its own copy. The answer is the agent's untrusted text;
// it only becomes a prompt for a new agent through the person's press.

/** One top-level item of a numbered list in an answer. */
export interface ListItem {
  /** The number the agent gave it. */
  n: number;
  /** Its first line without Markdown marks, for the pick list. */
  title: string;
  /** Its whole text, sub-points included, as the agent wrote it. */
  body: string;
}

const ITEM = /^(\d{1,3})[.)][ \t]+(\S.*)$/;
const FENCE = /^ {0,3}(```|~~~)/;

/**
 * The items of the answer's longest numbered list (the last of equally long
 * ones), or none when it has fewer than two. Only lists at the left edge
 * count: a nested list is part of its item, a list in a code block is code.
 */
export function listItems(text: string): ListItem[] {
  const lists: ListItem[][] = [];
  let list: ListItem[] | null = null;
  let item: { n: number; lines: string[] } | null = null;
  let blank = false;
  let fence: string | null = null;

  const closeItem = () => {
    if (item && list) list.push(toItem(item.n, item.lines));
    item = null;
  };
  const closeList = () => {
    closeItem();
    if (list && list.length > 0) lists.push(list);
    list = null;
  };

  for (const line of text.split(/\r?\n/)) {
    const mark = FENCE.exec(line)?.[1];
    if (fence !== null) {
      // Inside a code block: nothing is a list item, and it ends only on its own mark.
      if (item) item.lines.push(line);
      if (mark === fence) fence = null;
      continue;
    }
    if (mark) {
      // A code block inside an item is part of it; one at the left edge after a gap ends the list.
      if (item && (/^\s/.test(line) || !blank)) item.lines.push(line);
      else closeList();
      fence = mark;
      blank = false;
      continue;
    }
    const start = ITEM.exec(line);
    if (start) {
      closeItem();
      list ??= [];
      item = { n: Number(start[1]), lines: [start[2] ?? ""] };
      blank = false;
      continue;
    }
    if (!line.trim()) {
      blank = true;
      if (item) item.lines.push("");
      continue;
    }
    // Indented lines, and lines straight after the item's text, still belong to it.
    if (item && (/^\s/.test(line) || !blank)) {
      item.lines.push(line);
      blank = false;
      continue;
    }
    closeList();
    blank = false;
  }
  closeList();

  let best: ListItem[] = [];
  for (const each of lists) if (each.length >= best.length) best = each;
  return best.length >= 2 ? best : [];
}

function toItem(n: number, lines: string[]): ListItem {
  while (lines.length > 0 && !lines[lines.length - 1]?.trim()) lines.pop();
  const indent = Math.min(
    ...lines
      .slice(1)
      .filter((l) => l.trim())
      .map((l) => /^\s*/.exec(l)?.[0].length ?? 0),
    Number.POSITIVE_INFINITY,
  );
  const rest = lines.slice(1).map((l) => (Number.isFinite(indent) ? l.slice(indent) : l));
  return { n, title: plain(lines[0] ?? ""), body: [lines[0] ?? "", ...rest].join("\n") };
}

/** A line without the Markdown marks that would show as noise in a pick list. */
function plain(line: string): string {
  return line
    .replace(/!?\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/(\*\*|__|\*|_|`)(.+?)\1/g, "$2")
    .replace(/`/g, "")
    .trim();
}

/** "1, 3 and 5": the picked numbers as the button says them. */
export function numbersText(ns: readonly number[]): string {
  if (ns.length <= 1) return ns.map(String).join("");
  return `${ns.slice(0, -1).join(", ")} and ${ns[ns.length - 1]}`;
}

/**
 * What the agent for `item` is told: the item quoted from the answer to
 * `question`, and that other agents do the other picked items.
 */
export function itemPrompt(question: string, item: ListItem, others: number): string {
  const asked = question.split("\n")[0]?.trim() || "the last task";
  const quoted = item.body
    .split("\n")
    .map((line) => (line ? `> ${line}` : ">"))
    .join("\n");
  const alone =
    others > 0
      ? `Other agents do ${others === 1 ? "another item" : `${others} other items`} of that list at the same time, so change only what this one needs.`
      : "Change only what this item needs.";
  return `Item ${item.n} of the answer to "${asked}":\n\n${quoted}\n\nDo this item. ${alone}`;
}
