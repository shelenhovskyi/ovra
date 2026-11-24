//! `fs.*`: files inside a project. Paths are relative to the project root,
//! use `/` separators and may not leave the root.

use serde::{Deserialize, Serialize};

use super::projects::ProjectId;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum FsRequest {
    /// Direct children of a directory (`""` is the root), dirs first, then by name.
    #[serde(rename = "fs.list_dir")]
    ListDir { project: ProjectId, path: String },
    #[serde(rename = "fs.read_file")]
    ReadFile { project: ProjectId, path: String },
    /// Writes a UTF-8 file. With `expected_mtime_ms`, fails with `conflict`
    /// if the file changed on disk since then.
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
}
