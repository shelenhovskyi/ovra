import { defineCollection } from "astro:content";
import { glob } from "astro/loaders";
import { docId } from "./lib/rewrite-links.mjs";

// The repository's own docs/ folder is the source; nothing is copied.
const docs = defineCollection({
  loader: glob({
    pattern: ["**/*.md", "!adr/0000-template.md"],
    base: "../docs",
    generateId: ({ entry }) => docId(entry),
  }),
});

export const collections = { docs };
