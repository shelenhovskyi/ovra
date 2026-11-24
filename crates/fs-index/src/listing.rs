//! Directory listings with `.gitignore` and git status applied.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use ignore::WalkBuilder;
use ovra_core::path::RelPath;
use ovra_core::protocol::fs::{DirEntry, DirListing, EntryKind, GitStatus};
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::Result;

use crate::git::{Child, Statuses};
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
    let (statuses, inside_untracked) = git.map(|g| g.children(path)).unwrap_or_default();
    let mut entries = match resolve::existing(root, path) {
        Ok((_, dir)) => on_disk(&dir, path, &statuses, inside_untracked)?,
        // A directory whose tracked files were all deleted still lists them.
        Err(e) if e.code == ErrorCode::NotFound && statuses.values().any(|c| c.deleted) => {
            Vec::new()
        }
        Err(e) => return Err(e),
    };
    let present: HashSet<String> = entries.iter().map(|e| e.name.clone()).collect();
    for (name, child) in &statuses {
        if child.deleted && !present.contains(name) {
            entries.push(DirEntry {
                name: name.clone(),
                kind: if child.dir {
                    EntryKind::Dir
                } else {
                    EntryKind::File
                },
                ignored: false,
                git: Some(GitStatus::Deleted),
                renamed_from: None,
            });
        }
    }
    entries.sort_by(|a, b| {
        (a.kind != EntryKind::Dir)
            .cmp(&(b.kind != EntryKind::Dir))
            .then_with(|| natural_cmp(&a.name, &b.name))
    });
    Ok(DirListing {
        path: path.to_string(),
        entries,
    })
}

fn on_disk(
    dir: &Path,
    path: &RelPath,
    statuses: &HashMap<String, Child>,
    inside_untracked: bool,
) -> Result<Vec<DirEntry>> {
    if !dir.is_dir() {
        return Err(ProtocolError::new(
            ErrorCode::BadRequest,
            format!("{path} is not a directory"),
        ));
    }
    let visible = visible_names(dir);
    let mut entries = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| io_error(path, &e))? {
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
        let child = statuses.get(&name);
        let git = match child {
            Some(child) => Some(child.status),
            None if inside_untracked && !ignored => Some(GitStatus::Untracked),
            None => None,
        };
        entries.push(DirEntry {
            renamed_from: child.and_then(|c| c.renamed_from.clone()),
            name,
            kind,
            ignored,
            git,
        });
    }
    Ok(entries)
}

/// Natural order: digit runs compare as numbers, the rest case-insensitively;
/// ties fall back to byte order so the order is total.
pub(crate) fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a, b);
    loop {
        let (Some(cx), Some(cy)) = (x.chars().next(), y.chars().next()) else {
            return x
                .is_empty()
                .cmp(&y.is_empty())
                .reverse()
                .then_with(|| a.cmp(b));
        };
        let order = if cx.is_ascii_digit() && cy.is_ascii_digit() {
            let (dx, rx) = split_digits(x);
            let (dy, ry) = split_digits(y);
            (x, y) = (rx, ry);
            let (tx, ty) = (dx.trim_start_matches('0'), dy.trim_start_matches('0'));
            tx.len().cmp(&ty.len()).then_with(|| tx.cmp(ty))
        } else {
            (x, y) = (&x[cx.len_utf8()..], &y[cy.len_utf8()..]);
            cx.to_lowercase().cmp(cy.to_lowercase())
        };
        if order != Ordering::Equal {
            return order;
        }
    }
}

fn split_digits(s: &str) -> (&str, &str) {
    s.split_at(s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order() {
        let mut names = vec![
            "module-10",
            "module-2",
            "a file.txt",
            "README.md",
            "module-1",
            "b",
            "B",
            "x007",
            "x7",
            "x10",
        ];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(
            names,
            [
                "a file.txt",
                "B",
                "b",
                "module-1",
                "module-2",
                "module-10",
                "README.md",
                "x007",
                "x7",
                "x10"
            ]
        );
    }
}
