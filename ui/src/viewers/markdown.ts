// Markdown to HTML for the viewer. Raw HTML in the source is escaped, and
// micromark drops URLs with unsafe schemes (javascript:, data: in links, ...).

import { micromark } from "micromark";
import { gfm, gfmHtml } from "micromark-extension-gfm";
import { slug } from "./links";

/**
 * Renders `text` and post-processes the result: headings get anchor ids, and
 * image sources move to `data-src` so the browser never fetches them; the
 * viewer loads project images through the core instead.
 */
export function renderMarkdown(text: string): string {
  const html = micromark(text, { extensions: [gfm()], htmlExtensions: [gfmHtml()] });
  const doc = new DOMParser().parseFromString(`<body>${html}</body>`, "text/html");
  const seen = new Map<string, number>();
  for (const heading of doc.querySelectorAll("h1, h2, h3, h4, h5, h6")) {
    const base = slug(heading.textContent ?? "");
    const count = seen.get(base) ?? 0;
    seen.set(base, count + 1);
    heading.id = count ? `${base}-${count}` : base;
  }
  // Task-list checkboxes are named after their item, so they are not read as just "checkbox".
  for (const box of doc.querySelectorAll('li > input[type="checkbox"]')) {
    const item = box.parentElement?.textContent?.trim();
    if (item) box.setAttribute("aria-label", item);
  }
  for (const img of doc.querySelectorAll("img[src]")) {
    img.setAttribute("data-src", img.getAttribute("src") ?? "");
    img.removeAttribute("src");
  }
  return doc.body.innerHTML;
}
