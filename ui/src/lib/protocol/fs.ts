import type { ProjectId } from "./projects";

export type FsRequest =
  | { method: "fs.list_dir"; params: { project: ProjectId; path: string } }
  | { method: "fs.read_file"; params: { project: ProjectId; path: string } }
  /**
   * Every non-ignored file (sorted, symlinks included, no dirs), a page at a
   * time. Keep it fresh with `fs.files_changed`; restart if `version` changes
   * between pages. `limit` defaults to and is capped at 50,000 (MAX_FILES_PAGE in Rust).
   */
  | { method: "fs.list_files"; params: { project: ProjectId; offset?: number; limit?: number | null } }
  | {
      method: "fs.write_file";
      params: { project: ProjectId; path: string; text: string; expected_mtime_ms: number | null };
    };

export type EntryKind = "file" | "dir" | "symlink";

export type GitStatus = "modified" | "added" | "deleted" | "renamed" | "untracked" | "conflicted";

/**
 * Listings are dirs first, then natural order by name. Tracked files deleted
 * from disk are listed with `git: "deleted"`. `renamed_from` is the old
 * project-relative path of a `renamed` entry, absent otherwise.
 */
export type DirEntry = {
  name: string;
  kind: EntryKind;
  ignored: boolean;
  git: GitStatus | null;
  renamed_from?: string;
};

export type DirListing = { path: string; entries: DirEntry[] };

export type FileBody =
  | { kind: "text"; text: string }
  | { kind: "binary"; mime: string; base64: string }
  | { kind: "too_large" };

export type FileContent = { path: string; size: number; mtime_ms: number; body: FileBody };

export type FsResponse =
  | { type: "fs.dir"; data: DirListing }
  | { type: "fs.file"; data: FileContent }
  | { type: "fs.written"; data: { mtime_ms: number } }
  | { type: "fs.files"; data: FileList };

/**
 * One page of `fs.list_files`; `total` counts the whole project. `version` 0
 * means the project is still being indexed: list again on the next
 * `fs.files_changed`, which arrives once indexing ends.
 */
export type FileList = { paths: string[]; total: number; version: number };

export type FsEvent =
  | { type: "fs.changed"; data: { project: ProjectId; paths: string[] } }
  | { type: "fs.git_changed"; data: { project: ProjectId } }
  | {
      type: "fs.files_changed";
      data: { project: ProjectId; added: string[]; removed: string[]; version: number };
    };
