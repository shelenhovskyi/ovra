// Whether the paths a tool call names lie inside a folder (the project root,
// or a worker's worktree). Decided from the text of the paths only: symlinks
// are not followed, so "inside" means "inside by name", and any path whose
// meaning depends on more than its text (a shell expansion, `..`, a
// drive-relative Windows path) is "unknown", never "inside".

/** `relative`: the path from the folder, when it is inside. */
export type PathScope =
  | { path: string; scope: "inside"; relative: string }
  | { path: string; scope: "outside" | "unknown" };

const PATH_KEYS = ["file_path", "notebook_path", "path"];

/**
 * `prefix`: `/` (Unix), `c:/` (drive) or `//server/share/` (UNC), lower case
 * where the OS ignores case. `parts` keep `.` and `..` as written.
 */
type Parsed = { prefix: string; parts: string[]; caseless: boolean };

/** `relative` for a path with no root; `null` when its text cannot tell where it is. */
function parse(path: string): Parsed | "relative" | null {
  if (/^[~$%]/.test(path)) return null;
  const unix = path.replace(/\\/g, "/");
  const split = (rest: string) => rest.split("/").filter((p) => p !== "");
  const unc = /^\/\/([^/]+)\/([^/]+)(\/|$)/.exec(unix);
  if (unc) {
    return { prefix: `//${unc[1]}/${unc[2]}/`.toLowerCase(), parts: split(unix.slice(unc[0].length)), caseless: true };
  }
  if (unix.startsWith("//")) return null;
  const drive = /^([a-zA-Z]):/.exec(unix);
  if (drive) {
    const rest = unix.slice(2);
    // `C:foo` is relative to the current folder of drive C.
    if (!rest.startsWith("/")) return null;
    return { prefix: `${drive[1]?.toLowerCase()}:/`, parts: split(rest), caseless: true };
  }
  if (unix.startsWith("/")) return { prefix: "/", parts: split(unix), caseless: false };
  return "relative";
}

/** Resolves `.` and `..`; `dotted` tells whether there were any. */
function resolve(parts: string[]): { parts: string[]; dotted: boolean } {
  const out: string[] = [];
  let dotted = false;
  for (const part of parts) {
    if (part === "." || part === "..") dotted = true;
    if (part === ".") continue;
    if (part === "..") out.pop();
    else out.push(part);
  }
  return { parts: out, dotted };
}

function scopeOf(path: string, folder: Parsed): PathScope {
  const parsed = parse(path);
  if (parsed === null) return { path, scope: "unknown" };
  let full: Parsed;
  if (parsed === "relative") {
    full = {
      ...folder,
      parts: [
        ...folder.parts,
        ...path
          .replace(/\\/g, "/")
          .split("/")
          .filter((p) => p !== ""),
      ],
    };
  } else if (parsed.prefix === "/" && folder.prefix !== "/") {
    // `\x` on Windows is relative to the current drive, which the text does not say.
    return { path, scope: "unknown" };
  } else {
    full = parsed;
  }
  const { parts, dotted } = resolve(full.parts);
  const same = (a: string, b: string) => (folder.caseless ? a.toLowerCase() === b.toLowerCase() : a === b);
  const inside =
    full.prefix === folder.prefix &&
    parts.length >= folder.parts.length &&
    folder.parts.every((part, i) => same(part, parts[i] ?? ""));
  if (!inside) return { path, scope: "outside" };
  // `..` can leave the folder through a symlink although the name stays inside.
  if (dotted) return { path, scope: "unknown" };
  const relative = parts.slice(folder.parts.length).join("/");
  return { path, scope: "inside", relative: relative === "" ? "." : relative };
}

/** The folder as compared: `null` when it is not an absolute path. */
function folderOf(folder: string): Parsed | null {
  const parsed = parse(folder);
  if (parsed === null || parsed === "relative") return null;
  return { ...parsed, parts: resolve(parsed.parts).parts };
}

/** One entry per path the tool input names. */
export function pathScopes(input: unknown, folder: string | null): PathScope[] {
  if (typeof input !== "object" || input === null) return [];
  const record = input as Record<string, unknown>;
  const base = folder === null ? null : folderOf(folder);
  return PATH_KEYS.flatMap((key): PathScope[] => {
    const path = record[key];
    if (typeof path !== "string" || path === "") return [];
    return [base ? scopeOf(path, base) : { path, scope: "unknown" }];
  });
}

/** Whether two absolute folders name the same place by text. */
export function sameFolder(a: string, b: string): boolean {
  const x = folderOf(a);
  const y = folderOf(b);
  if (!x || !y || x.prefix !== y.prefix || x.parts.length !== y.parts.length) return false;
  return x.parts.every((part, i) =>
    x.caseless ? part.toLowerCase() === y.parts[i]?.toLowerCase() : part === y.parts[i],
  );
}
