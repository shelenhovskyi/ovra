//! The set of non-ignored files behind `fs.list_files`.

use std::collections::BTreeSet;
use std::path::{Component, Path};

use ignore::WalkBuilder;
use ovra_core::protocol::fs::FileList;

/// A walk of `dir` that honours ignore files and skips `.git`.
pub(crate) fn walk(dir: &Path) -> ignore::Walk {
    WalkBuilder::new(dir)
        .hidden(false)
        .require_git(false)
        .follow_links(false)
        .filter_entry(|e| e.file_name() != ".git")
        .build()
}

/// `path` relative to `root` with `/` separators, if it is under the root
/// and UTF-8.
pub(crate) fn relative(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let mut parts = Vec::new();
    for part in rel.components() {
        let Component::Normal(part) = part else {
            return None;
        };
        parts.push(part.to_str()?);
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// Every non-ignored file (or symlink) under a canonical root.
pub(crate) fn scan(root: &Path) -> BTreeSet<String> {
    walk(root)
        .filter_map(std::result::Result::ok)
        .filter(|e| e.file_type().is_some_and(|t| !t.is_dir()))
        .filter_map(|e| relative(root, e.path()))
        .collect()
}

pub(crate) fn page(files: &BTreeSet<String>, offset: u32, limit: u32, version: u64) -> FileList {
    FileList {
        paths: files
            .iter()
            .skip(offset as usize)
            .take(limit as usize)
            .cloned()
            .collect(),
        total: u32::try_from(files.len()).unwrap_or(u32::MAX),
        version,
    }
}
