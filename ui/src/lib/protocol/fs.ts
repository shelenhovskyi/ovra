import type { ProjectId } from "./projects";

export type FsRequest =
  | { method: "fs.list_dir"; params: { project: ProjectId; path: string } }
  | { method: "fs.read_file"; params: { project: ProjectId; path: string } }
  | {
      method: "fs.write_file";
      params: { project: ProjectId; path: string; text: string; expected_mtime_ms: number | null };
    };

export type EntryKind = "file" | "dir" | "symlink";

export type GitStatus = "modified" | "added" | "deleted" | "renamed" | "untracked" | "conflicted";

export type DirEntry = { name: string; kind: EntryKind; ignored: boolean; git: GitStatus | null };

export type DirListing = { path: string; entries: DirEntry[] };

export type FileBody =
  | { kind: "text"; text: string }
  | { kind: "binary"; mime: string; base64: string }
  | { kind: "too_large" };

export type FileContent = { path: string; size: number; mtime_ms: number; body: FileBody };

export type FsResponse =
  | { type: "fs.dir"; data: DirListing }
  | { type: "fs.file"; data: FileContent }
  | { type: "fs.written"; data: { mtime_ms: number } };

export type FsEvent =
  | { type: "fs.changed"; data: { project: ProjectId; paths: string[] } }
  | { type: "fs.git_changed"; data: { project: ProjectId } };
