// Whether the paths a tool call names lie inside the project folder. Decided
// from the text of the paths only: symlinks are not followed, so "inside"
// means "inside by name".

export type PathScope = { path: string; inside: boolean };

const PATH_KEYS = ["file_path", "notebook_path", "path"];

/** Forward slashes, `.` and `..` resolved; relative paths are taken from `root`. */
function normalize(path: string, root: string): string {
  const unix = path.replace(/\\/g, "/");
  const absolute = unix.startsWith("/") || /^[a-zA-Z]:\//.test(unix);
  const full = absolute ? unix : `${root.replace(/\\/g, "/")}/${unix}`;
  const drive = /^[a-zA-Z]:/.exec(full)?.[0] ?? "";
  const parts: string[] = [];
  for (const part of full.slice(drive.length).split("/")) {
    if (part === "" || part === ".") continue;
    if (part === "..") parts.pop();
    else parts.push(part);
  }
  const joined = `${drive}/${parts.join("/")}`;
  // Windows drive paths compare without case.
  return drive ? joined.toLowerCase() : joined;
}

export function pathScopes(input: unknown, root: string): PathScope[] {
  if (typeof input !== "object" || input === null) return [];
  const record = input as Record<string, unknown>;
  const base = normalize(root, "/");
  return PATH_KEYS.flatMap((key) => {
    const path = record[key];
    if (typeof path !== "string" || path === "") return [];
    const target = normalize(path, root);
    const inside = target === base || target.startsWith(base.endsWith("/") ? base : `${base}/`);
    return [{ path, inside }];
  });
}
