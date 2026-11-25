import { type CollectionEntry, getCollection } from "astro:content";

export type Doc = CollectionEntry<"docs">;

export interface Group {
  title: string;
  docs: { id: string; title: string }[];
}

/** The first `# Heading` of a document, or its file name. */
export function docTitle(doc: Doc): string {
  return doc.body?.match(/^#\s+(.+)$/m)?.[1]?.replace(/`/g, "") ?? doc.id;
}

const GROUPS: { title: string; match: (id: string) => boolean }[] = [
  { title: "Overview", match: (id) => !id.includes("/") && id !== "adr" && id !== "specs" && id !== "ux-reviews" },
  { title: "Decisions (ADRs)", match: (id) => id === "adr" || id.startsWith("adr/") },
  { title: "Specs", match: (id) => id === "specs" || id.startsWith("specs/") },
  { title: "Design", match: (id) => id === "design" || id.startsWith("design/") },
  { title: "UX reviews", match: (id) => id === "ux-reviews" || id.startsWith("ux-reviews/") },
];

const ORDER = ["architecture", "glossary"];
const rank = (id: string): number => {
  const i = ORDER.indexOf(id);
  return i === -1 ? ORDER.length : i;
};

/** Every document, grouped for the sidebar and the docs index. */
export async function docGroups(): Promise<Group[]> {
  const all = await getCollection("docs");
  return GROUPS.map((g) => ({
    title: g.title,
    docs: all
      .filter((d) => g.match(d.id))
      .sort((a, b) => rank(a.id) - rank(b.id) || a.id.localeCompare(b.id))
      .map((d) => ({ id: d.id, title: docTitle(d) })),
  })).filter((g) => g.docs.length > 0);
}
