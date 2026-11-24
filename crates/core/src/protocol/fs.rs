//! `fs.*`: files inside a project. Paths are relative to the project root,
//! use `/` separators and may not leave the root.

use serde::{Deserialize, Serialize};

use super::projects::ProjectId;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum FsRequest {
    /// Direct children of a directory (`""` is the root), dirs first, then by
    /// name in natural order (`a2` before `a10`, case-insensitive). Tracked
    /// files deleted from disk are listed too, with `git: deleted`; so is a
    /// directory that only holds such files.
    #[serde(rename = "fs.list_dir")]
    ListDir { project: ProjectId, path: String },
    #[serde(rename = "fs.read_file")]
    ReadFile { project: ProjectId, path: String },
    /// Every file of the project that is not ignored (as project-relative
    /// paths, sorted by byte order), a page at a time. Directories and
    /// `.git` are left out; symlinks are included. Keep the list fresh with
    /// `fs.files_changed`; if `version` differs between pages, start over.
    #[serde(rename = "fs.list_files")]
    ListFiles {
        project: ProjectId,
        #[serde(default)]
        offset: u32,
        /// Defaults to and is capped at [`MAX_FILES_PAGE`].
        #[serde(default)]
        limit: Option<u32>,
    },
    /// Writes a UTF-8 file. With `expected_mtime_ms`, fails with `conflict`
    /// if the file changed on disk since then or no longer exists.
    #[serde(rename = "fs.write_file")]
    WriteFile {
        project: ProjectId,
        path: String,
        text: String,
        expected_mtime_ms: Option<u64>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum FsResponse {
    #[serde(rename = "fs.dir")]
    Dir(DirListing),
    #[serde(rename = "fs.file")]
    File(FileContent),
    #[serde(rename = "fs.written")]
    Written { mtime_ms: u64 },
    #[serde(rename = "fs.files")]
    Files(FileList),
}

/// The largest page `fs.list_files` returns.
pub const MAX_FILES_PAGE: u32 = 50_000;

/// One page of `fs.list_files`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileList {
    pub paths: Vec<String>,
    /// Number of files in the whole project.
    pub total: u32,
    /// Changes at least whenever the set of files changes. `0` means the
    /// project is still being indexed after `projects.open`: the list is a
    /// fresh walk, and an `fs.files_changed` follows once indexing ends;
    /// list again then.
    pub version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirListing {
    pub path: String,
    pub entries: Vec<DirEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirEntry {
    pub name: String,
    pub kind: EntryKind,
    /// Matched by `.gitignore` and friends. Ignored entries are still listed.
    pub ignored: bool,
    pub git: Option<GitStatus>,
    /// For `git: renamed`, the project-relative path it was renamed from.
    /// Left out otherwise, and when the old path is outside the project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub renamed_from: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    File,
    Dir,
    Symlink,
}

/// Git status of a file, or the "strongest" status inside a directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitStatus {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
    Conflicted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileContent {
    pub path: String,
    pub size: u64,
    pub mtime_ms: u64,
    pub body: FileBody,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FileBody {
    Text {
        text: String,
    },
    /// Images and other binaries, base64-encoded. Large files are not sent.
    Binary {
        mime: String,
        base64: String,
    },
    TooLarge,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum FsEvent {
    /// Files or directories changed on disk (by people, agents or git).
    #[serde(rename = "fs.changed")]
    Changed {
        project: ProjectId,
        paths: Vec<String>,
    },
    /// Git status changed for some paths; clients re-fetch the affected dirs.
    #[serde(rename = "fs.git_changed")]
    GitChanged { project: ProjectId },
    /// Files were added to or removed from the `fs.list_files` set.
    /// `version` is the set's new version.
    #[serde(rename = "fs.files_changed")]
    FilesChanged {
        project: ProjectId,
        added: Vec<String>,
        removed: Vec<String>,
        version: u64,
    },
}
