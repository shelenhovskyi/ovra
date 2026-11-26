// Where a link in what an agent said goes when the person clicks it.

import { resolveLink } from "../viewers/links";

export type ChatLink = { kind: "web"; url: string } | { kind: "file"; path: string } | null;

/**
 * `projectRoot` and `sessionRoot` are absolute folders. Web pages open in the
 * system browser; a file opens in the editor only when the session works in
 * the project's own checkout, since a worktree's copy is not the file the
 * editor would show. A line suffix (`:12`, `:12:3`) is dropped.
 */
export function chatLink(href: string, projectRoot: string | null, sessionRoot: string | null): ChatLink {
  if (/^https?:\/\//i.test(href)) {
    const link = resolveLink("", href);
    return link.kind === "web" ? link : null;
  }
  if (!projectRoot || sessionRoot !== projectRoot) return null;
  let target = href.replace(/:\d+(:\d+)?$/, "");
  if (target.startsWith("/")) {
    if (!target.startsWith(`${projectRoot}/`)) return null;
    target = target.slice(projectRoot.length + 1);
  }
  const link = resolveLink("", target);
  return link.kind === "file" ? link : null;
}
