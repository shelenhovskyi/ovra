// Hover and completion docs are Markdown that the language server writes and
// the client renders to HTML; a project's server is not trusted with the
// page. Only plain text markup survives: an allowlist of tags, `class`, and
// `href` for http and https links (which the editor opens through the core,
// never in this window).

const HTML_NS = "http://www.w3.org/1999/xhtml";

const ALLOWED = new Set(
  "a b blockquote br code del em h1 h2 h3 h4 h5 h6 hr i li ol p pre s span strong table tbody td th thead tr ul".split(
    " ",
  ),
);

/** Dropped with everything inside them; other unknown elements keep their children. */
const DROPPED = new Set(
  "script style svg math iframe object embed template noscript textarea select title head img video audio picture canvas input link meta base".split(
    " ",
  ),
);

/** `href` if it is an absolute http or https URL, else null. */
export function safeHref(href: string | null): string | null {
  if (href === null) return null;
  try {
    const url = new URL(href);
    return url.protocol === "http:" || url.protocol === "https:" ? href : null;
  } catch {
    return null;
  }
}

/** `html` with only allowlisted tags and attributes. */
export function sanitizeHtml(html: string): string {
  // A parsed document is inert: nothing in it runs or loads.
  const source = new DOMParser().parseFromString(html, "text/html");
  const out = source.createElement("div");
  copy(source.body, out);
  return out.innerHTML;
}

/**
 * Only the classes code highlighting gives (`tok-*`, CodeMirror's generated
 * `ͼ*`, `language-*`): any other would let a server borrow the app's own
 * styles and draw fake app UI in a tooltip.
 */
function safeClass(value: string | null): string | null {
  const kept = (value ?? "").split(/\s+/).filter((c) => /^(tok-[\w-]+|\u037c\w+|language-[\w+#-]+)$/.test(c));
  return kept.length > 0 ? kept.join(" ") : null;
}

function copy(from: Node, to: Element): void {
  const doc = to.ownerDocument;
  for (const node of from.childNodes) {
    if (node.nodeType === Node.TEXT_NODE) {
      to.appendChild(doc.createTextNode(node.textContent ?? ""));
      continue;
    }
    // Comments, and elements in the SVG or MathML namespaces, are dropped.
    if (!(node instanceof Element) || node.namespaceURI !== HTML_NS) continue;
    const tag = node.localName;
    if (DROPPED.has(tag)) continue;
    if (!ALLOWED.has(tag)) {
      copy(node, to);
      continue;
    }
    const clean = doc.createElement(tag);
    const cls = safeClass(node.getAttribute("class"));
    if (cls) clean.setAttribute("class", cls);
    const href = tag === "a" ? safeHref(node.getAttribute("href")) : null;
    if (href) {
      clean.setAttribute("href", href);
      // Where it goes, since the system browser opens it in one click.
      clean.setAttribute("title", href);
    }
    to.appendChild(clean);
    copy(node, clean);
    // A link's text can claim anything: name the site it opens unless the text does.
    if (href) {
      const host = new URL(href).host;
      if (!(clean.textContent ?? "").includes(host)) to.appendChild(doc.createTextNode(` (${host})`));
    }
  }
}
