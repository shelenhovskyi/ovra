// Resolves links inside a rendered document relative to the file that holds them.

export type Link =
  | { kind: "file"; path: string }
  | { kind: "anchor"; id: string }
  | { kind: "web"; url: string }
  | { kind: "external" };

/**
 * `from` is the project-relative path of the document. Returns where `href`
 * points: another project file, an anchor in the same document, a web page
 * for the system browser, or somewhere the viewer does not go (other
 * schemes, paths above the root).
 */
export function resolveLink(from: string, href: string): Link {
  if (href.startsWith("#")) return { kind: "anchor", id: decodeURIComponent(href.slice(1)) };
  if (/^https?:\/\/[^/]/i.test(href)) return { kind: "web", url: href };
  if (/^[a-z][a-z0-9+.-]*:/i.test(href) || href.startsWith("//")) return { kind: "external" };
  const target = href.split(/[?#]/)[0] ?? "";
  if (target === "") return { kind: "external" };
  const parts = target.startsWith("/") ? [] : from.split("/").slice(0, -1);
  for (const part of decodeURIComponent(target).split("/")) {
    if (part === "" || part === ".") continue;
    if (part === "..") {
      if (parts.length === 0) return { kind: "external" };
      parts.pop();
    } else parts.push(part);
  }
  return parts.length ? { kind: "file", path: parts.join("/") } : { kind: "external" };
}

/** GitHub-style heading anchor: lower case, spaces to dashes, punctuation dropped. */
export function slug(text: string): string {
  return text
    .trim()
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\s_-]/gu, "")
    .replace(/\s/g, "-");
}
