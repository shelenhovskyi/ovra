// Rewrites links in the repository's rendered Markdown so they work on the site:
// a link to another file under docs/ goes to that page, a link to any other
// file in the repository goes to GitHub.
import { dirname, relative, resolve } from "node:path";

// Builds run from site/ (pnpm --filter @ovra/site build).
const REPO = resolve(process.cwd(), "..");
const DOCS = resolve(REPO, "docs");
const BASE = "/ovra";
export const GITHUB = "https://github.com/shelenhovskyi/ovra";

/** docs/adr/README.md -> "adr", docs/adr/0001-x.md -> "adr/0001-x". */
export function docId(rel) {
  return rel
    .replace(/\.md$/i, "")
    .replace(/(^|\/)readme$/i, "")
    .toLowerCase();
}

/** Where a link written in `file` (absolute path) should point on the site. */
function siteHref(href, file) {
  if (/^([a-z]+:|#|\/)/i.test(href)) return href;
  const [path, hash = ""] = href.split("#");
  const target = resolve(dirname(file), path);
  const anchor = hash ? `#${hash}` : "";
  const inDocs = relative(DOCS, target);
  if (!inDocs.startsWith("..") && /\.md$/i.test(target)) {
    const id = docId(inDocs);
    return `${BASE}/docs/${id ? `${id}/` : ""}${anchor}`;
  }
  const inRepo = relative(REPO, target);
  if (inRepo.startsWith("..")) return href;
  return `${GITHUB}/blob/main/${inRepo}${anchor}`;
}

/** Rewrites every relative href in `html`, rendered from `file` (path relative to the repository). */
export function rewriteHtml(html, file) {
  const abs = resolve(REPO, file);
  return html.replace(/href="([^"]*)"/g, (_, href) => `href="${siteHref(href.replaceAll("&amp;", "&"), abs)}"`);
}
