// Files are named `ovra:///<path in the project>` towards language servers,
// each segment percent-encoded (docs/specs/lsp.md, "URIs"). The core turns
// them into real `file:` URIs and back; a `file:` URI that reaches the UI is
// outside the project and is never opened.

const PREFIX = "ovra:///";

/** The project root as a URI. */
export const ROOT_URI = PREFIX;

/** The URI for a project-relative path. */
export function uriOf(path: string): string {
  return PREFIX + path.split("/").map(encodeURIComponent).join("/");
}

/** The project-relative path a URI names, or null for anything outside the project. */
export function pathOf(uri: string): string | null {
  if (!uri.startsWith(PREFIX) || uri.length === PREFIX.length) return null;
  try {
    return uri.slice(PREFIX.length).split("/").map(decodeURIComponent).join("/");
  } catch {
    return null;
  }
}
