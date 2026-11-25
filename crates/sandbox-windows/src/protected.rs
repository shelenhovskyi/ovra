//! The protected paths of one session's folder, found on disk: the shared
//! list of `ovra-sandbox`, every `.git` (the project's and nested
//! repositories'), and the folder `core.hooksPath` names. The sandbox
//! denies them every write.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use ovra_core::services::Result;

use crate::Setup;
use crate::invalid;
use crate::plan::real;

/// A walk for `**/` names stops after this many entries.
const WALK_LIMIT: usize = 500_000;

/// Every protected path in `work_dir` that exists, after making the
/// missing folders of `made`, so that the agent cannot create one. Sorted,
/// without paths inside another one. A protected path that is a link or a
/// junction refuses the session: its name could be replaced.
pub(crate) fn paths(setup: &Setup, work_dir: &Path, made: &[&str]) -> Result<Vec<PathBuf>> {
    let mut found = vec![work_dir.join(".git")];
    let mut deep = vec![".git"];
    for pattern in setup.protected {
        match pattern.strip_prefix("**/") {
            Some(name) => deep.push(name),
            None => found.push(work_dir.join(pattern)),
        }
    }
    let hidden = [real(&setup.data_dir).unwrap_or_else(|_| setup.data_dir.clone())];
    walk(work_dir, &deep, &hidden, &mut found)?;
    let mut folders: Vec<PathBuf> = made
        .iter()
        .filter(|name| setup.protected.contains(name))
        .map(|name| work_dir.join(name))
        .collect();
    folders.extend(hooks_path(work_dir, &setup.home));
    for folder in folders {
        if let Some(folder) = made_inside(work_dir, &folder)? {
            found.push(folder);
        }
    }
    let mut protected = Vec::new();
    for path in found {
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if is_link(&meta) {
            return Err(invalid(format!(
                "{} is a link: the sandbox cannot keep it read-only. Replace it with the file itself to run agents here",
                path.display()
            )));
        }
        protected.push(real(&path)?);
    }
    protected.sort();
    let mut kept: Vec<PathBuf> = Vec::new();
    for path in protected {
        if !kept.iter().any(|k| path.starts_with(k)) {
            kept.push(path);
        }
    }
    Ok(kept)
}

/// A symbolic link, or on Windows any reparse point (a junction is not a
/// symbolic link to `std`).
fn is_link(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    meta.file_type().is_symlink()
}

/// The folder `core.hooksPath` names, if set and inside `work_dir`.
fn hooks_path(work_dir: &Path, home: &Path) -> Option<PathBuf> {
    // Never anything from the repository's config that runs a program.
    let out = Command::new("git")
        .args(["-c", "core.fsmonitor=false", "--no-optional-locks"])
        .args(["config", "--get", "core.hooksPath"])
        .current_dir(work_dir)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8(out.stdout).ok()?;
    let text = text.trim();
    if !out.status.success() || text.is_empty() {
        return None;
    }
    let path = match text.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => work_dir.join(text),
    };
    lexically_inside(&path, work_dir).then_some(path)
}

/// Whether `path` stays inside `dir` without following anything.
fn lexically_inside(path: &Path, dir: &Path) -> bool {
    use std::path::Component;
    let Ok(rest) = path.strip_prefix(dir) else {
        return false;
    };
    let mut depth = 0usize;
    for part in rest.components() {
        match part {
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            Component::ParentDir if depth > 0 => depth -= 1,
            _ => return false,
        }
    }
    depth > 0
}

/// Makes the folder `path` if it is missing, only through real folders
/// inside `work_dir`, and returns what to protect: `path`, or the first
/// part of it that is not a folder.
fn made_inside(work_dir: &Path, path: &Path) -> Result<Option<PathBuf>> {
    let Ok(rest) = path.strip_prefix(work_dir) else {
        return Ok(None);
    };
    let mut at = work_dir.to_path_buf();
    let parts: Vec<_> = rest.components().collect();
    for (n, part) in parts.iter().enumerate() {
        at.push(part);
        match fs::symlink_metadata(&at) {
            Ok(meta) if meta.is_dir() && !is_link(&meta) && n + 1 < parts.len() => {}
            Ok(_) => return Ok(Some(at)),
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                return Err(invalid(format!(
                    "cannot make {} for the sandbox to keep read-only: {e}",
                    at.display()
                )));
            }
            Err(_) => fs::create_dir(&at).map_err(|e| {
                invalid(format!(
                    "cannot make {} for the sandbox to keep read-only: {e}",
                    at.display()
                ))
            })?,
        }
    }
    Ok(Some(at))
}

/// Adds every entry under `dir` named one of `names` (case ignored, as
/// Windows does), without following links or entering `.git` or `hidden`.
/// A folder too large to look through refuses the session: what the walk
/// did not see would stay writable.
fn walk(dir: &Path, names: &[&str], hidden: &[PathBuf], found: &mut Vec<PathBuf>) -> Result<()> {
    let mut stack = vec![dir.to_path_buf()];
    let mut seen = 0;
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            seen += 1;
            if seen > WALK_LIMIT {
                return Err(invalid(format!(
                    "{} holds more than {WALK_LIMIT} files: too many for the sandbox to look through",
                    dir.display()
                )));
            }
            let name = entry.file_name();
            let path = entry.path();
            if names.iter().any(|n| name.eq_ignore_ascii_case(n)) {
                found.push(path.clone());
            }
            let Ok(meta) = fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.is_dir()
                && !is_link(&meta)
                && !name.eq_ignore_ascii_case(".git")
                && !hidden.iter().any(|h| path.starts_with(h))
            {
                stack.push(path);
            }
        }
    }
    Ok(())
}
