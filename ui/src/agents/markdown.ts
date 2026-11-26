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
  const html = micromark(text, { extensions: [gfm()], htmlExtensions: [gfmHtml()] });
  const doc = new DOMParser().parseFromString(`<body>${html}</body>`, "text/html");
  for (const img of doc.body.querySelectorAll("img")) {
    const alt = img.getAttribute("alt") ?? "";
    img.replaceWith(alt ? `[${alt}]` : "");
  }
  clean(doc.body);
  return doc.body.innerHTML;
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
      const href = node.getAttribute("href") ?? "";
      // Shown on hover, so the person sees where a link really goes.
      if (href) node.setAttribute("title", href);
      else node.removeAttribute("href");
    }
    clean(node);
  }
}
