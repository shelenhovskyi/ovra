//! Directory listings with `.gitignore` and git status applied.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use ignore::WalkBuilder;
use ovra_core::path::RelPath;
use ovra_core::protocol::fs::{DirEntry, DirListing, EntryKind, GitStatus};
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::Result;

use crate::git::Statuses;
use crate::resolve::{self, io_error};

/// Names of the direct children of `dir` that `.gitignore`, `.ignore`,
/// `.git/info/exclude` and the global excludes file do not exclude.
/// Rules apply in any folder, inside a git repository or not.
pub(crate) fn visible_names(dir: &Path) -> HashSet<String> {
    WalkBuilder::new(dir)
        .max_depth(Some(1))
        .hidden(false)
        .require_git(false)
        .follow_links(false)
        .build()
        .filter_map(std::result::Result::ok)
        .filter(|e| e.depth() == 1)
        .filter_map(|e| e.file_name().to_str().map(str::to_owned))
        .collect()
}

pub(crate) fn list(root: &Path, path: &RelPath, git: Option<&Statuses>) -> Result<DirListing> {
    let (_, dir) = resolve::existing(root, path)?;
    if !dir.is_dir() {
        return Err(ProtocolError::new(
            ErrorCode::BadRequest,
            format!("{path} is not a directory"),
        ));
    }
    let visible = visible_names(&dir);
    let (statuses, inside_untracked) = git.map(|g| g.children(path)).unwrap_or_default();
    let mut entries = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|e| io_error(path, &e))? {
        let entry = entry.map_err(|e| io_error(path, &e))?;
        // Names that are not UTF-8 cannot be addressed by the protocol.
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let file_type = entry.file_type().map_err(|e| io_error(path, &e))?;
        let kind = if file_type.is_symlink() {
            EntryKind::Symlink
        } else if file_type.is_dir() {
            EntryKind::Dir
        } else {
            EntryKind::File
        };
        let ignored = name == ".git" || !visible.contains(&name);
        let git = match statuses.get(&name) {
            Some(status) => Some(*status),
            None if inside_untracked && !ignored => Some(GitStatus::Untracked),
            None => None,
        };
        entries.push(DirEntry {
            name,
            kind,
            ignored,
            git,
        });
    }
    entries.sort_by(|a, b| {
        (a.kind != EntryKind::Dir)
            .cmp(&(b.kind != EntryKind::Dir))
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(DirListing {
        path: path.to_string(),
        entries,
    })
}
