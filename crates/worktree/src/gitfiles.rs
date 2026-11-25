//! The files outside any review diff that change what git runs: every
//! config file git reads (the repository's, its includes, the user's and
//! the system's), `config.worktree`, `info/attributes`, `info/exclude`,
//! submodules' config and hooks, and the hooks folder (SEC-5). An agent
//! without a sandbox can plant a hook that runs on the person's next
//! `git commit`, outside any agent's reach.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use ovra_core::services::{GitFiles, Result};
use sha2::{Digest, Sha256};

use crate::git::{self, Dirs, Git};

/// Files read per hooks folder besides git's own hook names. More show as
/// one entry with their count, so a flood of files is a change too.
const MAX_HOOKS: usize = 200;

/// Submodules whose config and hooks are read, at most.
const MAX_MODULES: usize = 50;

/// The hooks git runs (`githooks(5)`): always read, whatever else a hooks
/// folder holds.
const HOOKS: [&str; 28] = [
    "applypatch-msg",
    "pre-applypatch",
    "post-applypatch",
    "pre-commit",
    "pre-merge-commit",
    "prepare-commit-msg",
    "commit-msg",
    "post-commit",
    "pre-rebase",
    "post-checkout",
    "post-merge",
    "pre-push",
    "pre-receive",
    "update",
    "proc-receive",
    "post-receive",
    "post-update",
    "reference-transaction",
    "push-to-checkout",
    "pre-auto-gc",
    "post-rewrite",
    "sendemail-validate",
    "fsmonitor-watchman",
    "p4-changelist",
    "p4-prepare-changelist",
    "p4-post-changelist",
    "p4-pre-submit",
    "post-index-change",
];

/// The files of `dir`'s repository. Call it on the person's checkout: a
/// worker's worktree has a `.git` file the worker could have pointed
/// elsewhere.
pub(crate) fn git_files(dir: &Path) -> Result<GitFiles> {
    let dirs = git::dirs(dir)?;
    // `core.hooksPath` if set, relative to the top of the working tree as
    // git runs hooks from there, else the common git folder's `hooks`.
    let hooks = Git::new(&dirs.top, "rev-parse")
        .args(["--path-format=absolute", "--git-path", "hooks"])
        .text()?;
    let hooks = PathBuf::from(hooks);
    let origins = Git::new(&dirs.top, "config")
        .args(["--list", "--show-origin", "--includes", "--name-only", "-z"])
        .run()?;
    let mut files = BTreeMap::new();
    let mut add = |path: &Path| {
        if let Some(version) = version(path) {
            files.insert(name(&dirs, path), version);
        }
    };
    add(&dirs.common.join("config"));
    add(&dirs.git_dir.join("config.worktree"));
    add(&dirs.common.join("info").join("attributes"));
    add(&dirs.common.join("info").join("exclude"));
    // Every config file git read, includes and the user's own among them.
    for origin in git::fields(&origins).step_by(2) {
        if let Some(path) = origin.strip_prefix("file:") {
            add(&dirs.top.join(path));
        }
    }
    let mut modules = Vec::new();
    submodules(&dirs.common.join("modules"), 3, &mut modules);
    for module in modules.iter().take(MAX_MODULES) {
        add(&module.join("config"));
        hooks_in(&module.join("hooks"), &mut add);
    }
    // Husky's own hooks folder (`.husky/_`) is ignored, so it is never in
    // the diff either: hooks in the working tree are read too.
    let more = hooks_in(&hooks, &mut add);
    if modules.len() > MAX_MODULES {
        let marker = dirs.common.join("modules");
        files.insert(
            format!("{} (more submodules)", name(&dirs, &marker)),
            modules.len().to_string(),
        );
    }
    if more > 0 {
        files.insert(
            format!("{} (more files)", name(&dirs, &hooks)),
            more.to_string(),
        );
    }
    let hooks_in_tree = (!hooks.starts_with(&dirs.common))
        .then(|| hooks.strip_prefix(&dirs.top).ok())
        .flatten()
        .map(slashed)
        .and_then(|rel| {
            // Relative to the project, as review paths are.
            let rel = format!("{rel}/");
            rel.strip_prefix(dirs.prefix.as_str())
                .map(|r| r.trim_end_matches('/').to_owned())
        })
        .filter(|rel| !rel.is_empty());
    Ok(GitFiles {
        hooks_in_tree,
        files,
    })
}

/// Reads git's own hook names in `dir`, then up to [`MAX_HOOKS`] other
/// files two folders deep (not git's `.sample`s, which never run). Returns
/// how many files it left out.
fn hooks_in(dir: &Path, add: &mut impl FnMut(&Path)) -> usize {
    for hook in HOOKS {
        add(&dir.join(hook));
        add(&dir.join(format!("{hook}.exe")));
    }
    let mut others = Vec::new();
    walk(dir, 2, &mut others);
    others.retain(|p| {
        let file = p.file_name().map(|n| n.to_string_lossy().into_owned());
        let file = file.unwrap_or_default();
        let stem = file.strip_suffix(".exe").unwrap_or(&file);
        let own = p.parent() == Some(dir) && HOOKS.contains(&stem);
        !own && !file.ends_with(".sample")
    });
    others.sort();
    for path in others.iter().take(MAX_HOOKS) {
        add(path);
    }
    others.len().saturating_sub(MAX_HOOKS)
}

/// Files under `dir`, `depth` folders deep, without following links to
/// folders.
fn walk(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for path in entries.filter_map(|e| e.ok().map(|e| e.path())) {
        if fs::symlink_metadata(&path).is_ok_and(|m| m.is_dir()) {
            if depth > 1 {
                walk(&path, depth - 1, out);
            }
        } else {
            out.push(path);
        }
    }
}

/// Git folders of submodules under `modules` (`modules/<name>`, nested ones
/// under `modules/<name>/modules`): those with a `config`.
fn submodules(modules: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(modules) else {
        return;
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| fs::symlink_metadata(p).is_ok_and(|m| m.is_dir()))
        .collect();
    dirs.sort();
    for dir in dirs {
        if dir.join("config").is_file() {
            out.push(dir.clone());
        }
        if depth > 1 {
            submodules(&dir, depth - 1, out);
            submodules(&dir.join("modules"), depth - 1, out);
        }
    }
}

/// What the file is: a hash of its content and whether it may run, and for
/// a link also its target (what it points to may change on its own). `None`
/// if there is no such file.
fn version(path: &Path) -> Option<String> {
    let meta = fs::symlink_metadata(path).ok()?;
    if meta.is_symlink() {
        let target = fs::read_link(path).ok()?;
        let content = fs::read(path).map_or_else(|_| "missing".to_owned(), |b| hash(&b));
        return Some(format!("link {} {content}", target.display()));
    }
    if !meta.is_file() {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    let kind = if runnable(&meta) { "exec" } else { "file" };
    Some(format!("{kind} {}", hash(&bytes)))
}

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
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
