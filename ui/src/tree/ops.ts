// Pure helpers for the tree's file actions: name checks, copy names, paths
// and the keys that start an action. The core checks every path again; these
// only give a quick answer while the person types.

import { isApple } from "../keys/shortcut";
import { joinPath, parentOf } from "./model";

/** The last part of a project-relative path. */
export function baseName(path: string): string {
  return path.slice(path.lastIndexOf("/") + 1);
}

/**
 * Why `name` cannot be used in a folder holding `siblings`, or null if it can.
 * `nested` allows `a/b/c.ts` (new files and folders); `current` is the name
 * being renamed, which may change only its case.
 */
export function nameProblem(
  name: string,
  siblings: { name: string; kind: string }[],
  opts: { nested: boolean; current?: string },
): string | null {
  if (name.trim() === "") return "Enter a name";
  if (name.startsWith("/") || name.endsWith("/")) return "No / at the start or end";
  const parts = name.split("/");
  if (!opts.nested && parts.length > 1) return "No / in a name";
  if (parts.some((p) => p === "" || p === "." || p === "..")) return "Not a valid name";
  if (parts.some((p) => p.toLowerCase() === ".git")) return "Not allowed: .git";
  const first = parts[0] as string;
  // Case-insensitive, as on macOS and Windows disks.
  const same = siblings.find((s) => s.name.toLowerCase() === first.toLowerCase());
  if (!same || (opts.current !== undefined && first.toLowerCase() === opts.current.toLowerCase())) return null;
  if (parts.length === 1) return `“${same.name}” exists`;
  return same.kind === "dir" ? null : `“${same.name}” is a file`;
}

/** The one question before `paths` go to the trash. */
export function trashQuestion(paths: string[]): string {
  return paths.length === 1
    ? `Move “${baseName(paths[0] as string)}” to Trash?`
    : `Move ${paths.length} items to Trash?`;
}

/** The part of `name` a rename selects first: all but the extension of a file. */
export function stemRange(name: string, isDir: boolean): [number, number] {
  const dot = name.lastIndexOf(".");
  return [0, isDir || dot <= 0 ? name.length : dot];
}

/** A free name for a copy of `name`: `a copy.ts`, then `a copy 2.ts`, ... */
export function copyName(name: string, isDir: boolean, siblings: string[]): string {
  const taken = new Set(siblings.map((s) => s.toLowerCase()));
  const [, end] = stemRange(name, isDir);
  const stem = name.slice(0, end);
  const ext = name.slice(end);
  for (let n = 1; ; n++) {
    const candidate = `${stem} copy${n === 1 ? "" : ` ${n}`}${ext}`;
    if (!taken.has(candidate.toLowerCase())) return candidate;
  }
}

/** Where `path` is after `from` moved to `to`, or null if the move did not touch it. */
export function movedPath(path: string, from: string, to: string): string | null {
  if (path === from) return to;
  return path.startsWith(`${from}/`) ? to + path.slice(from.length) : null;
}

/** Whether `path` is `dir` or lies inside it. */
function isWithin(path: string, dir: string): boolean {
  return path === dir || path.startsWith(`${dir}/`);
}

/** Where moving `from` into the folder `dir` puts it, or null when that is no move or impossible. */
export function moveTarget(from: string, dir: string): string | null {
  if (parentOf(from) === dir || isWithin(dir, from)) return null;
  return joinPath(dir, baseName(from));
}

type Os = "mac" | "windows" | "linux";

function os(): Os {
  if (isApple()) return "mac";
  return typeof navigator !== "undefined" && /Win/.test(navigator.platform || navigator.userAgent)
    ? "windows"
    : "linux";
}

/** The menu label for showing a file in the system file manager. */
export function revealLabel(system: Os = os()): string {
  if (system === "mac") return "Reveal in Finder";
  return system === "windows" ? "Reveal in File Explorer" : "Open Containing Folder";
}

/** The trash shortcut, as menus show it. */
export function trashKeys(apple = isApple()): string {
  return apple ? "Mod+Backspace" : "Delete";
}

type Keys = Pick<KeyboardEvent, "code" | "ctrlKey" | "metaKey" | "altKey" | "shiftKey">;

/** Which file action a key on the tree asks for, matched on the physical key. */
export function keyAction(e: Keys, apple = isApple()): "rename" | "trash" | null {
  const mods = e.ctrlKey || e.metaKey || e.altKey || e.shiftKey;
  if (e.code === "F2" && !mods) return "rename";
  if (apple && e.code === "Backspace" && e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey) return "trash";
  if (e.code === "Delete" && !mods) return "trash";
  return null;
}
