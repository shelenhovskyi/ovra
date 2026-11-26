// Markdown in what an agent says, as HTML the transcript can show. The text
// comes from the agent, so it is untrusted: micromark escapes raw HTML and
// drops unsafe URLs, and on top of that only an allowlist of tags and
// attributes survives. Images are never fetched: each becomes its alt text.

import { micromark } from "micromark";
import { gfm, gfmHtml } from "micromark-extension-gfm";

const TAGS = new Set([
  "p",
  "br",
  "hr",
  "h1",
  "h2",
  "h3",
  "h4",
  "h5",
  "h6",
  "strong",
  "em",
  "del",
  "code",
  "pre",
  "blockquote",
  "ul",
  "ol",
  "li",
  "a",
  "table",
  "thead",
  "tbody",
  "tr",
  "th",
  "td",
  "input",
]);
const ATTRS: Record<string, readonly string[]> = {
  a: ["href"],
  code: ["class"],
  ol: ["start"],
  th: ["align"],
  td: ["align"],
  input: ["type", "checked", "disabled"],
};

/** Renders an agent's Markdown `text` to sanitized HTML. */
export function renderChat(text: string): string {
  const html = micromark(text, {
    extensions: [gfm()],
    htmlExtensions: [gfmHtml()],
  });
  const doc = new DOMParser().parseFromString(`<body>${html}</body>`, "text/html");
  for (const img of doc.body.querySelectorAll("img")) {
    const alt = img.getAttribute("alt") ?? "";
    img.replaceWith(alt ? `[${alt}]` : "");
  }
  clean(doc.body);
  nestHeadings(doc.body);
  return doc.body.innerHTML;
}

/**
 * The agent's headings sit under the task's own (an h3), so they never
 * join the page outline at the app's levels: `#` becomes an h4, `##` an h5,
 * the rest h6. A class keeps how big each one looks.
 */
function nestHeadings(body: HTMLElement): void {
  for (const old of body.querySelectorAll("h1, h2, h3, h4, h5, h6")) {
    const level = Number(old.localName.slice(1));
    const heading = old.ownerDocument.createElement(`h${Math.min(level + 3, 6)}`);
    heading.className = `md-h${level}`;
    heading.append(...old.childNodes);
    old.replaceWith(heading);
  }
}

/** The words of an agent's Markdown `text`, without its marks: what a screen reader says. */
export function plainChat(text: string): string {
  const doc = new DOMParser().parseFromString(`<body>${renderChat(text)}</body>`, "text/html");
  // Blocks and cells end with a space, so their words never run together.
  for (const block of doc.body.querySelectorAll("p, li, h1, h2, h3, h4, h5, h6, pre, blockquote, th, td")) {
    block.append(" ");
  }
  return (doc.body.textContent ?? "").replace(/\s+/g, " ").trim();
}

function clean(parent: Element): void {
  for (const node of [...parent.childNodes]) {
    if (node.nodeType === Node.TEXT_NODE) continue;
    if (!(node instanceof Element) || !TAGS.has(node.localName)) {
      // Unknown elements keep their text, never their markup; comments go.
      node.replaceWith(node instanceof Element ? (node.textContent ?? "") : "");
      continue;
    }
    const allowed = ATTRS[node.localName] ?? [];
    for (const attr of [...node.attributes]) {
      if (!allowed.includes(attr.name)) node.removeAttribute(attr.name);
    }
    if (node.localName === "input") {
      if (node.getAttribute("type") !== "checkbox") {
        node.remove();
        continue;
      }
      node.setAttribute("disabled", "");
    }
    if (node.localName === "code" && !/^language-[\w+#.-]+$/.test(node.getAttribute("class") ?? "")) {
      node.removeAttribute("class");
    }
    if (node.localName === "a") {
      // Only links Ovra would follow keep a target: web pages and plain paths,
      // not mailto:, irc: and the like, which a middle click could hand the OS.
      const href = node.getAttribute("href") ?? "";
      const web = /^https?:\/\//i.test(href);
      const path = !/^[a-z][a-z0-9+.-]*:|^\/\//i.test(href);
      // Shown on hover, so the person sees where a link really goes.
      if (href && (web || path)) node.setAttribute("title", href);
      else node.removeAttribute("href");
    }
    clean(node);
  }
}
