// Where a link in what an agent said goes when the person clicks it.

import { resolveLink } from "../viewers/links";

export type ChatLink = { kind: "web"; url: string } | { kind: "file"; path: string; line: number | null } | null;

/**
 * `projectRoot` and `sessionRoot` are absolute folders. Web pages open in the
 * system browser; a file opens in the editor only when the session works in
 * the project's own checkout, since a worktree's copy is not the file the
 * editor would show. A line suffix (`:12`, `:12:3`, `#L12`) names the line.
 */
export function chatLink(href: string, projectRoot: string | null, sessionRoot: string | null): ChatLink {
  if (/^https?:\/\//i.test(href)) {
    const link = resolveLink("", href);
    return link.kind === "web" ? link : null;
  }
  if (!projectRoot || sessionRoot !== projectRoot) return null;
  let target: string;
  try {
    target = decodeURIComponent(href);
  } catch {
    return null;
  }
  const spot = /(?::(\d+)(?::\d+)?|#L(\d+))$/.exec(target);
  const line = spot ? Number(spot[1] ?? spot[2]) : null;
  if (spot) target = target.slice(0, spot.index);
  // Other schemes (javascript:, file:, C:) and host-relative links go nowhere.
  if (/^[a-z][a-z0-9+.-]*:|^\/\/|\\/i.test(target)) return null;
  if (target.startsWith("/")) {
    if (!target.startsWith(`${projectRoot}/`)) return null;
    target = target.slice(projectRoot.length + 1);
  }
  // resolveLink decodes again and stops at ? and #: hand it the path encoded.
  const link = resolveLink("", target.split("/").map(encodeURIComponent).join("/"));
  return link.kind === "file" ? { ...link, line } : null;
}
