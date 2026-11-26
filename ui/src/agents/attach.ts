// Files the person drops on or pastes into a message box become @ chips
// (docs/specs/task-context.md, "Dropped and pasted files"). A file of the
// project is only named, as when it is picked with @. A file from elsewhere
// is copied into the project's attachments folder first, which git
// ignores, because the agent reads only the project: the drop or the paste
// is the person's explicit action, as a drop on the file tree is.

import { PATHS_DRAG_TYPE } from "../lib/drag";
import type { ContextWhat, ProjectId } from "../lib/protocol";
import type { Transport, WindowDrop } from "../lib/transport";

/** A pasted or dropped file can be this large at most (the core's limit). */
const MAX_BYTES = 10 * 1024 * 1024;

/** `path` relative to the folder `root`, or null outside it. Separators may be `/` or `\`. */
export function relativeTo(root: string, path: string): string | null {
  const norm = (p: string) => p.replaceAll("\\", "/").replace(/\/+$/, "");
  const [base, full] = [norm(root), norm(path)];
  // Windows paths compare without case.
  const windows = /^[a-z]:\//i.test(base);
  const prefix = `${base}/`;
  const inside = windows ? full.toLowerCase().startsWith(prefix.toLowerCase()) : full.startsWith(prefix);
  if (!inside) return null;
  const rel = full.slice(prefix.length);
  return rel.split("/").some((part) => part === "" || part === "." || part === "..") ? null : rel;
}

/** A project path as an item: a folder when the core can list it, else a file. */
export async function itemFor(transport: Transport, project: ProjectId, path: string): Promise<ContextWhat> {
  try {
    await transport.call({ method: "fs.list_dir", params: { project, path } });
    return { kind: "folder", path };
  } catch {
    return { kind: "file", path };
  }
}

/** A drop's paths relative to `root` when all are inside it, else null (it needs a copy). */
export function insidePaths(root: string, drop: WindowDrop): string[] | null {
  const inside = drop.paths.map((path) => relativeTo(root, path));
  return inside.every((path) => path !== null) ? (inside as string[]) : null;
}

/** Copies a drop into the attachments folder (as one: the drop is one id); the copies' paths. */
export async function copyDrop(transport: Transport, project: ProjectId, drop: WindowDrop): Promise<string[]> {
  if (!transport.importDrop) throw new Error("Files from outside the project cannot be added here.");
  const dir = await transport.call({ method: "context.attachments", params: { project } });
  const done = await transport.importDrop(drop.id, project, dir.data.path, "keep_both");
  return done.kind === "copied" ? done.paths : [];
}

/**
 * The names among `paths` (absolute, or bare file names) whose copy would
 * be a secret store, by the core's chip check: those wait for the person's
 * "Copy anyway", since a copy in the project is one every agent can read.
 * The file's folder counts too (`.ssh/id_ed25519`).
 */
export async function secretLooking(transport: Transport, project: ProjectId, paths: string[]): Promise<string[]> {
  if (paths.length === 0) return [];
  const tail = (path: string) => {
    const parts = path
      .replaceAll("\\", "/")
      .split("/")
      .filter((p) => p && p !== "." && p !== "..");
    return parts.slice(-2).join("/") || "file";
  };
  const items = paths.map((path) => ({ kind: "file" as const, path: `${ATTACHMENTS}/${tail(path)}` }));
  // The core checks 20 items a call at most.
  const notes: unknown[] = [];
  for (let at = 0; at < items.length; at += 20) {
    const batch = items.slice(at, at + 20);
    const answer = await transport.call({ method: "context.check", params: { project, items: batch } });
    notes.push(...answer.data.notes);
  }
  return paths.filter((_, i) => notes[i] != null).map(baseName);
}

function baseName(path: string): string {
  return path.replaceAll("\\", "/").replace(/\/+$/, "").split("/").at(-1) ?? path;
}

/** The project's attachments folder (the core's `context.attachments`). */
const ATTACHMENTS = ".ovra-attachments";

/** Saves pasted or dropped files in the attachments folder; their project paths. */
export async function attachFiles(transport: Transport, project: ProjectId, files: File[]): Promise<string[]> {
  const paths: string[] = [];
  for (const file of files) {
    if (file.size > MAX_BYTES) throw new Error(`${file.name || "The file"} is larger than 10 MB.`);
    const data = await base64(file);
    const name = file.name || `pasted.${file.type.split("/")[1]?.replace(/[^a-z0-9]/gi, "") || "bin"}`;
    const saved = await transport.call({ method: "context.attach", params: { project, name, data } });
    paths.push(saved.data.path);
  }
  return paths;
}

function base64(file: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const url = String(reader.result);
      resolve(url.slice(url.indexOf(",") + 1));
    };
    reader.onerror = () => reject(reader.error ?? new Error("The file could not be read."));
    reader.readAsDataURL(file);
  });
}

/** The files a paste carries, unless it carries text to paste as text (a name copied with its file is not that). */
export function pastedFiles(data: DataTransfer | null): File[] {
  const files = [...(data?.files ?? [])];
  if (files.length === 0) return [];
  const text = data?.getData("text/plain").trim() ?? "";
  return text === "" || files.some((file) => text.endsWith(file.name)) ? files : [];
}

export type DropHandlers = {
  /** Files are over the box (true) or left it (false). */
  over(on: boolean): void;
  /** Files dropped onto the window over the box (the desktop app). */
  window(drop: WindowDrop): void;
  /** File tree rows: project paths. */
  rows(paths: string[]): void;
  /** Files the page got without paths (a browser). */
  files(files: File[]): void;
};

/**
 * Makes `box` take files: from the file tree, from the system's file
 * manager (the window reports those to the desktop app, with their paths)
 * and, in a browser, as the page's own drops. Returns the cleanup.
 */
export function dropZone(box: HTMLElement, transport: Transport, on: DropHandlers): () => void {
  const hits = (at: { x: number; y: number }) => {
    const el = document.elementFromPoint?.(at.x, at.y);
    return el !== null && el !== undefined && box.contains(el);
  };
  let overNow = false;
  const over = (next: boolean) => {
    if (next === overNow) return;
    overNow = next;
    on.over(next);
  };
  const offOver = transport.onDragOver?.((at) => over(at !== null && hits(at)));
  const offDrop = transport.onDrop?.((drop) => {
    const mine = hits(drop);
    over(false);
    if (mine) on.window(drop);
  });
  // The desktop app reports files from outside itself; the page sees only rows.
  const pageFiles = transport.onDrop === undefined;
  const takes = (e: DragEvent) => {
    const types = [...(e.dataTransfer?.types ?? [])];
    return types.includes(PATHS_DRAG_TYPE) || (pageFiles && types.includes("Files"));
  };
  const dragover = (e: DragEvent) => {
    if (!takes(e)) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "copy";
    over(true);
  };
  const dragleave = (e: DragEvent) => {
    if (!(e.relatedTarget instanceof Node && box.contains(e.relatedTarget))) over(false);
  };
  const drop = (e: DragEvent) => {
    if (!takes(e)) return;
    e.preventDefault();
    over(false);
    const rows = e.dataTransfer?.getData(PATHS_DRAG_TYPE) ?? "";
    if (rows) on.rows(rows.split("\n").filter(Boolean));
    else on.files([...(e.dataTransfer?.files ?? [])]);
  };
  box.addEventListener("dragenter", dragover);
  box.addEventListener("dragover", dragover);
  box.addEventListener("dragleave", dragleave);
  box.addEventListener("drop", drop);
  return () => {
    offOver?.();
    offDrop?.();
    box.removeEventListener("dragenter", dragover);
    box.removeEventListener("dragover", dragover);
    box.removeEventListener("dragleave", dragleave);
    box.removeEventListener("drop", drop);
  };
}
