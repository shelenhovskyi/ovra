//! The repository's own files that no review diff shows but that change
//! what git runs: config, `info/attributes`, `info/exclude` and the hooks
//! folder (SEC-5). An agent without a sandbox can plant a hook that runs on
//! the person's next `git commit`, outside any agent's reach.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use ovra_core::services::{GitFiles, Result};
use sha2::{Digest, Sha256};

use crate::git::{self, Dirs, Git};

/// Hook files read at most. A folder an agent fills with more still shows
/// as changed: the files read differ.
const MAX_HOOKS: usize = 200;

pub(crate) fn git_files(dir: &Path) -> Result<GitFiles> {
    let dirs = git::dirs(dir)?;
    // `core.hooksPath` if set (relative to the top of the working tree),
    // else the common git folder's `hooks`.
    let hooks = Git::new(dir, "rev-parse")
        .args(["--path-format=absolute", "--git-path", "hooks"])
        .text()?;
    let hooks = PathBuf::from(hooks);
    let mut files = BTreeMap::new();
    let mut add = |path: &Path| {
        if let Some(version) = version(path) {
            files.insert(name(&dirs, path), version);
        }
    };
    add(&dirs.common.join("config"));
    if dirs.git_dir != dirs.common {
        add(&dirs.git_dir.join("config.worktree"));
    }
    add(&dirs.common.join("info").join("attributes"));
    add(&dirs.common.join("info").join("exclude"));
    // Husky's own hooks folder (`.husky/_`) is ignored, so it is never in
    // the diff either: hooks in the working tree are read too.
    let mut left = MAX_HOOKS;
    walk(&hooks, 2, &mut left, &mut add);
    let hooks_in_tree = (!hooks.starts_with(&dirs.common))
        .then(|| hooks.strip_prefix(&dirs.top).ok())
        .flatten()
        .map(slashed)
        .filter(|rel| !rel.is_empty());
    Ok(GitFiles {
        hooks_in_tree,
        files,
    })
}

/// Files under `dir`, `depth` folders deep, except git's `.sample` hooks,
/// which never run.
fn walk(dir: &Path, depth: u32, left: &mut usize, add: &mut impl FnMut(&Path)) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    for path in paths {
        if *left == 0 {
            return;
        }
        if path.extension().is_some_and(|e| e == "sample") {
            continue;
        }
        let is_dir = fs::symlink_metadata(&path).is_ok_and(|m| m.is_dir());
        if is_dir {
            if depth > 1 {
                walk(&path, depth - 1, left, add);
            }
            continue;
        }
        *left -= 1;
        add(&path);
    }
}

/// What the file is: its link target, or a hash of its content and
/// whether it may run. `None` if there is no such file.
fn version(path: &Path) -> Option<String> {
    let meta = fs::symlink_metadata(path).ok()?;
    if meta.is_symlink() {
        let target = fs::read_link(path).ok()?;
        return Some(format!("link {}", target.display()));
    }
    if !meta.is_file() {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    let digest = Sha256::digest(&bytes);
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    Some(format!(
        "{} {hex}",
        if runnable(&meta) { "exec" } else { "file" }
    ))
}

#[cfg(unix)]
fn runnable(meta: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn runnable(_: &fs::Metadata) -> bool {
    false
}

/// How the review names `path`: `.git/...` inside the git folder, relative
/// inside the working tree, else as it is.
fn name(dirs: &Dirs, path: &Path) -> String {
    if let Ok(rel) = path.strip_prefix(&dirs.common) {
        return format!(".git/{}", slashed(rel));
    }
    if let Ok(rel) = path.strip_prefix(&dirs.top) {
        return slashed(rel);
    }
    path.display().to_string()
}

fn slashed(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}
