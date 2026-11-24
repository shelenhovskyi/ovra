// Picks how the main area shows a file in view mode.

import type { FileBody } from "../lib/protocol";

export type View =
  | { kind: "markdown"; text: string }
  | { kind: "image"; src: string }
  | { kind: "patch"; text: string }
  | { kind: "text"; text: string }
  | { kind: "binary"; mime: string }
  | { kind: "too_large" };

const MARKDOWN = new Set(["md", "markdown", "mdown", "mkd"]);
const PATCH = new Set(["diff", "patch"]);

function extension(path: string): string {
  const name = path.slice(path.lastIndexOf("/") + 1);
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(dot + 1).toLowerCase() : "";
}

export function viewFor(path: string, body: FileBody): View {
  if (body.kind === "too_large") return { kind: "too_large" };
  if (body.kind === "binary") {
    if (body.mime.startsWith("image/")) return { kind: "image", src: `data:${body.mime};base64,${body.base64}` };
    return { kind: "binary", mime: body.mime };
  }
  const ext = extension(path);
  // SVG arrives as text. Inside <img> it cannot run scripts or load resources.
  if (ext === "svg") return { kind: "image", src: `data:image/svg+xml;charset=utf-8,${encodeURIComponent(body.text)}` };
  if (MARKDOWN.has(ext)) return { kind: "markdown", text: body.text };
  if (PATCH.has(ext)) return { kind: "patch", text: body.text };
  return { kind: "text", text: body.text };
}
