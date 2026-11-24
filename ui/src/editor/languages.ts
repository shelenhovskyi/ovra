// Languages are data packages in `languages/<id>/` (see languages/README.md).

import { grammarUrls } from "./grammars";
import type { LanguageSource } from "./highlight/protocol";

type Manifest = {
  name: string;
  extensions: string[];
  filenames?: string[];
  grammar: string;
  highlights: string[];
};

export type Language = Manifest & { id: string };

const ROOT = "../../../languages/";

const manifests = import.meta.glob<Manifest>("../../../languages/*/language.json", {
  eager: true,
  import: "default",
});
const queries = import.meta.glob<string>("../../../languages/*/*.scm", { query: "?raw", import: "default" });

export const languages: readonly Language[] = Object.entries(manifests).map(([file, manifest]) => ({
  id: file.slice(ROOT.length).split("/")[0] ?? file,
  ...manifest,
}));

/** The language for a project-relative path, or null for plain text. */
export function languageFor(path: string): Language | null {
  const name = path.slice(path.lastIndexOf("/") + 1);
  const byName = languages.find((l) => l.filenames?.includes(name));
  if (byName) return byName;
  const dot = name.lastIndexOf(".");
  if (dot <= 0) return null;
  const ext = name.slice(dot + 1).toLowerCase();
  return languages.find((l) => l.extensions.includes(ext)) ?? null;
}

/** What the highlight worker needs: the grammar URL and the query texts. */
export async function languageSource(language: Language): Promise<LanguageSource> {
  const grammar = grammarUrls[language.grammar];
  if (!grammar) throw new Error(`no grammar bundled for ${language.grammar}`);
  const texts = await Promise.all(
    language.highlights.map((path) => {
      const load = queries[ROOT + path];
      if (!load) throw new Error(`missing highlight query ${path}`);
      return load();
    }),
  );
  return { id: language.id, grammar, queries: texts };
}
