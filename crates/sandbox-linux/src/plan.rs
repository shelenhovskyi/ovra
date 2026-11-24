//! What one session's sandbox shows: the mounts bubblewrap makes, in order,
//! and what Landlock lets the helper write. Later mounts cover earlier
//! ones, so the order is: hide, bind back, then the protected paths
//! read-only on top.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{CommandLine, ProfileNetwork, Result, SandboxProfile};

use crate::helper::Landlock;
use crate::{HELPER_FLAG, Setup, invalid, os};

/// The protected paths of `ovra-sandbox`, handed over by it (it depends on
/// this crate, not the other way round).
#[derive(Debug, Clone, Copy)]
pub struct Protected {
    /// Under `work_dir`; `**/name` is any depth.
    pub work_dir: &'static [&'static str],
    /// Under a worker's own `<git dir>/worktrees/<name>`.
    pub worktree_git_dir: &'static [&'static str],
}

/// Folders under `~/.claude` that `claude` writes during a session. Each
/// session gets them empty and private: the user's own sessions read them
/// too (shell snapshots are sourced before each Bash call), so an agent
/// must not change theirs. They are made if missing, because `~/.claude`
/// itself stays read-only.
const CLAUDE_PRIVATE: [&str; 14] = [
    "backups",
    "cache",
    "debug",
    "file-history",
    "ide",
    "logs",
    "paste-cache",
    "plans",
    "session-env",
    "sessions",
    "shell-snapshots",
    "statsig",
    "telemetry",
    "todos",
];

/// Files `claude` rewrites: its login (the known limit in the spec) and its
/// prompt history. The history is made if missing.
const CLAUDE_FILES: [&str; 2] = [".credentials.json", "history.jsonl"];

/// Package caches builds write, relative to the home folder; each is made
/// if its parent exists.
const CACHES: [&str; 5] = [
    ".cargo/registry",
    ".cargo/git",
    ".npm",
    ".cache/pip",
    ".cache/pnpm",
];

/// Files that stay readable from the home folder. Not all of
/// `~/.config/git`: git's credential store keeps `credentials` there.
const HOME_READ: [&str; 4] = [
    ".gitconfig",
    ".config/git/config",
    ".config/git/ignore",
    ".config/git/attributes",
];

/// Secrets inside readable folders of the home folder: covered by
/// `/dev/null`.
const SECRETS: [&str; 3] = ["credentials", "credentials.toml", "credentials.json"];

/// Folders under the home folder whose `bin` on `PATH` brings the whole
/// folder (the toolchain's libraries live next to it). Any other `bin` is
/// bound alone: `~/.docker`, `~/.pulumi` or `~/.fly` keep tokens next to it.
const TOOLCHAINS: [&str; 12] = [
    ".cargo",
    ".rustup",
    ".nvm",
    ".volta",
    ".bun",
    ".deno",
    "go",
    ".pyenv",
    ".rbenv",
    ".sdkman",
    ".npm-global",
    ".yarn",
];

/// Folders under the home folder a program's own folder is never bound
/// from, whatever their depth.
const NEVER_BOUND: [&str; 7] = [
    ".config",
    ".ssh",
    ".aws",
    ".gnupg",
    ".docker",
    ".kube",
    ".local/share/keyrings",
];

/// The port inside the sandbox where the helper bridges the netproxy.
pub(crate) const PROXY_PORT: u16 = 3128;

/// A walk for `**/` names stops after this many entries.
const WALK_LIMIT: usize = 200_000;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Mount {
    /// Bound read-only at the same path.
    Ro(PathBuf),
    /// Bound writable at the same path.
    Rw(PathBuf),
    /// An empty private folder.
    Tmpfs(PathBuf),
    /// `/dev/null` over a file.
    Null(PathBuf),
}

/// The sandbox of one session.
#[derive(Debug)]
pub(crate) struct Plan {
    home: PathBuf,
    mounts: Vec<Mount>,
    /// The folders and files Landlock lets the agent write.
    writable: Vec<PathBuf>,
    network: ProfileNetwork,
    path: OsString,
}

impl Plan {
    pub(crate) fn new(setup: &Setup, profile: &SandboxProfile) -> Result<Self> {
        let home = real(&setup.home)?;
        let work_dir = real(&profile.work_dir)?;
        if !work_dir.is_dir() {
            return Err(invalid(format!("{} is not a folder", work_dir.display())));
        }
        let exposes_home = |p: &Path| home.starts_with(real(p).unwrap_or_else(|_| p.to_path_buf()));
        for path in std::iter::once(&work_dir)
            .chain(&profile.read)
            .chain(&profile.write)
        {
            if exposes_home(path) {
                return Err(invalid(format!(
                    "the sandbox does not let an agent into {}: it holds the home folder",
                    path.display()
                )));
            }
        }
        // `claude`'s own configuration stays read-only, whatever the project.
        let claude = (home.join(".claude"), home.join(".claude.json"));
        for path in std::iter::once(&work_dir).chain(&profile.write) {
            let path = real(path).unwrap_or_else(|_| path.clone());
            if path.starts_with(&claude.0) || path == claude.1 {
                return Err(invalid(format!(
                    "the sandbox does not let an agent write {}: it holds claude's settings",
                    path.display()
                )));
            }
        }
        let mut plan = Self {
            home: home.clone(),
            mounts: Vec::new(),
            writable: vec!["/tmp".into(), "/dev".into(), home.clone()],
            network: profile.network.clone(),
            path: OsString::new(),
        };
        plan.hide(setup, &home);
        let shown: Vec<PathBuf> = std::iter::once(work_dir.clone())
            .chain(
                profile
                    .read
                    .iter()
                    .chain(&profile.write)
                    .filter_map(|p| real(p).ok()),
            )
            .collect();
        plan.home_back(setup, &home, &work_dir, &shown);
        for read in &profile.read {
            if let Ok(read) = real(read) {
                plan.mounts.push(Mount::Ro(read));
            }
        }
        for write in &profile.write {
            plan.rw(real(write)?);
        }
        let worker = worker_git_dir(&work_dir, profile)?;
        if let Some(git_dir) = &worker {
            plan.rw(git_dir.clone());
        }
        plan.rw(work_dir.clone());
        let hidden: Vec<PathBuf> = setup.hidden.iter().filter_map(|h| real(h).ok()).collect();
        let protected = protected_paths(setup, &work_dir, worker.as_deref(), &home, &hidden)?;
        plan.protect(protected);
        // Ovra's data folder inside a folder bound above (a project that
        // holds `.ovra-data`) is hidden again.
        for hidden in hidden {
            if !work_dir.starts_with(&hidden) && plan.under_writable(&hidden).is_some() {
                plan.mounts.push(Mount::Tmpfs(hidden));
            }
        }
        if let ProfileNetwork::Proxy { address } = &profile.network {
            plan.mounts.push(Mount::Ro(real(Path::new(address))?));
        }
        Ok(plan)
    }

    /// Binds `path` read-only unless a folder bound before already shows
    /// it: binding it again would cover what was bound inside it since.
    fn ro(&mut self, path: PathBuf) {
        let shown = self.mounts.iter().any(|m| match m {
            Mount::Ro(p) | Mount::Rw(p) => path.starts_with(p),
            Mount::Tmpfs(_) | Mount::Null(_) => false,
        });
        if !shown {
            self.mounts.push(Mount::Ro(path));
        }
    }

    fn rw(&mut self, path: PathBuf) {
        self.writable.push(path.clone());
        self.mounts.push(Mount::Rw(path));
    }

    /// `/tmp`, `/run` (keeping the resolver's file), the home folder and
    /// the hidden folders become empty.
    fn hide(&mut self, setup: &Setup, home: &Path) {
        self.mounts.push(Mount::Tmpfs("/tmp".into()));
        self.mounts.push(Mount::Tmpfs("/run".into()));
        if let Ok(resolv) = fs::canonicalize("/etc/resolv.conf")
            && resolv.starts_with("/run")
        {
            self.mounts.push(Mount::Ro(resolv));
        }
        self.mounts.push(Mount::Tmpfs(home.to_path_buf()));
        for hidden in &setup.hidden {
            if let Ok(hidden) = real(hidden)
                && hidden.is_dir()
                && !hidden.starts_with(home)
            {
                self.mounts.push(Mount::Tmpfs(hidden));
            }
        }
    }

    /// What `claude`, git and the toolchains need from the home folder.
    fn home_back(&mut self, setup: &Setup, home: &Path, work_dir: &Path, shown: &[PathBuf]) {
        let claude = home.join(".claude");
        let _ = fs::create_dir_all(&claude);
        if let Ok(claude) = real(&claude)
            && claude.starts_with(home)
        {
            self.mounts.push(Mount::Ro(claude.clone()));
            for dir in CLAUDE_PRIVATE {
                let dir = claude.join(dir);
                if fs::create_dir_all(&dir).is_ok()
                    && let Ok(dir) = real(&dir)
                    && dir.starts_with(&claude)
                {
                    self.writable.push(dir.clone());
                    self.mounts.push(Mount::Tmpfs(dir));
                }
            }
            // The transcripts of this folder's sessions, which a resume
            // reads; not those of the user's other projects.
            let transcripts = claude.join("projects").join(project_key(work_dir));
            if fs::create_dir_all(&transcripts).is_ok()
                && let Ok(transcripts) = real(&transcripts)
                && transcripts.starts_with(&claude)
            {
                self.rw(transcripts);
            }
            for file in CLAUDE_FILES {
                let file = claude.join(file);
                if file.ends_with("history.jsonl") {
                    let _ = fs::OpenOptions::new().create(true).append(true).open(&file);
                }
                if let Ok(meta) = fs::symlink_metadata(&file)
                    && meta.is_file()
                {
                    self.rw(file);
                }
            }
        }
        let state = home.join(".claude.json");
        if fs::symlink_metadata(&state).is_ok_and(|m| m.is_file()) {
            self.rw(state);
        }
        for read in HOME_READ {
            if let Ok(read) = real(&home.join(read))
                && read.starts_with(home)
            {
                self.ro(read);
            }
        }
        for toolchain in &setup.toolchains {
            if let Ok(toolchain) = real(toolchain)
                && toolchain.is_dir()
                && !home.starts_with(&toolchain)
            {
                self.ro(toolchain);
            }
        }
        self.path = self.path_entries(setup, home, shown);
        // Secrets and caches inside the folders bound above come after them.
        let bound: Vec<PathBuf> = self
            .mounts
            .iter()
            .filter_map(|m| match m {
                Mount::Ro(p) if p.starts_with(home) && p.is_dir() => Some(p.clone()),
                _ => None,
            })
            .collect();
        for folder in bound {
            for secret in SECRETS {
                let secret = folder.join(secret);
                if fs::symlink_metadata(&secret).is_ok_and(|m| m.is_file())
                    && let Ok(secret) = real(&secret)
                {
                    self.mounts.push(Mount::Null(secret));
                }
            }
        }
        for cache in CACHES {
            let cache = home.join(cache);
            // `~/.npm` only if it exists; `~/.cargo/registry` if `~/.cargo` does.
            let parent = cache.parent().filter(|p| *p != home && p.is_dir());
            if (cache.is_dir() || parent.is_some())
                && fs::create_dir_all(&cache).is_ok()
                && let Ok(cache) = real(&cache)
                && cache.starts_with(home)
            {
                self.rw(cache);
            }
        }
    }

    /// The sandbox's `PATH`: Ovra's, without relative entries and without
    /// the home folder or its ancestors. Entries under the home folder are
    /// bound read-only: a toolchain's `bin` with its toolchain, any other
    /// alone. Entries inside the session's folders are shown anyway.
    fn path_entries(&mut self, setup: &Setup, home: &Path, shown: &[PathBuf]) -> OsString {
        let mut kept = Vec::new();
        for entry in std::env::split_paths(&setup.path) {
            if !entry.is_absolute() || home.starts_with(&entry) {
                continue;
            }
            if entry.starts_with(home)
                && let Ok(real_entry) = real(&entry)
                && !shown.iter().any(|s| real_entry.starts_with(s))
            {
                let toolchain = real_entry.parent().filter(|parent| {
                    real_entry.ends_with("bin")
                        && parent
                            .strip_prefix(home)
                            .ok()
                            .and_then(|rest| rest.components().next())
                            .is_some_and(|top| TOOLCHAINS.iter().any(|t| top.as_os_str() == *t))
                });
                let bound = toolchain.map_or_else(|| real_entry.clone(), Path::to_path_buf);
                if bound.starts_with(home) && !home.starts_with(&bound) {
                    self.ro(bound);
                }
            }
            kept.push(entry);
        }
        std::env::join_paths(kept).unwrap_or_default()
    }

    /// The writable mount `path` is under, if any.
    fn under_writable(&self, path: &Path) -> Option<PathBuf> {
        self.mounts
            .iter()
            .filter_map(|m| match m {
                Mount::Rw(root) if path.starts_with(root) => Some(root.clone()),
                _ => None,
            })
            .max_by_key(|root| root.components().count())
    }

    /// Binds each protected path read-only. Its folders below the writable
    /// mount it is in are bound onto themselves first: a mount point cannot
    /// be renamed, so the agent cannot move a protected path away and put
    /// its own in its place.
    fn protect(&mut self, paths: Vec<PathBuf>) {
        let mut pinned: Vec<PathBuf> = Vec::new();
        let mut covered: Vec<PathBuf> = Vec::new();
        for path in paths {
            let Some(root) = self.under_writable(&path) else {
                continue;
            };
            if path == root || covered.contains(&path) {
                continue;
            }
            let mut folder = path.parent();
            while let Some(dir) = folder.filter(|d| *d != root && d.starts_with(&root)) {
                if !pinned.iter().any(|p| p == dir) {
                    pinned.push(dir.to_path_buf());
                }
                folder = dir.parent();
            }
            covered.push(path);
        }
        pinned.sort_by_key(|p| p.components().count());
        self.mounts.extend(pinned.into_iter().map(Mount::Rw));
        self.mounts.extend(covered.into_iter().map(Mount::Ro));
    }

    /// The `bwrap` line that runs `line` through the helper.
    pub(crate) fn line(
        &self,
        bwrap: &Path,
        helper: &Path,
        landlock: Landlock,
        line: CommandLine,
    ) -> Result<CommandLine> {
        let program = real(&line.program)?;
        let helper = real(helper)?;
        let cwd = real(&line.cwd)?;
        let mut args: Vec<OsString> = [
            "--unshare-user",
            "--unshare-pid",
            "--unshare-ipc",
            "--unshare-uts",
            "--unshare-cgroup-try",
            "--die-with-parent",
            "--new-session",
            "--cap-drop",
            "ALL",
        ]
        .iter()
        .map(os)
        .collect();
        if !matches!(self.network, ProfileNetwork::Open) {
            args.push(os("--unshare-net"));
        }
        for flag in ["--ro-bind", "/", "/", "--dev", "/dev", "--proc", "/proc"] {
            args.push(os(flag));
        }
        let mut mounts = self.mounts.clone();
        // The helper and the program, wherever they are installed.
        mounts.push(Mount::Ro(helper.clone()));
        mounts.push(Mount::Ro(self.program_root(&program)));
        nothing_lost(&mounts)?;
        for mount in &mounts {
            let (flag, from, to) = match mount {
                Mount::Ro(p) => ("--ro-bind", p.as_os_str(), p),
                Mount::Rw(p) => ("--bind", p.as_os_str(), p),
                Mount::Null(p) => ("--ro-bind", "/dev/null".as_ref(), p),
                Mount::Tmpfs(p) => {
                    args.extend([os("--tmpfs"), os(p)]);
                    continue;
                }
            };
            args.extend([os(flag), os(from), os(to)]);
        }
        args.extend([
            os("--chdir"),
            os(&cwd),
            os("--"),
            os(&helper),
            os(HELPER_FLAG),
        ]);
        args.extend([os("--landlock"), os(landlock.as_arg())]);
        for path in &self.writable {
            args.extend([os("--rw"), os(path)]);
        }
        let mut env = line.env;
        if let ProfileNetwork::Proxy { address } = &self.network {
            args.extend([os("--bridge"), os(real(Path::new(address))?)]);
            args.push(os(PROXY_PORT.to_string()));
            let proxy = os(format!("http://127.0.0.1:{PROXY_PORT}"));
            for name in ["HTTP_PROXY", "HTTPS_PROXY", "http_proxy", "https_proxy"] {
                env.push((os(name), Some(proxy.clone())));
            }
            for name in ["NO_PROXY", "no_proxy", "ALL_PROXY", "all_proxy"] {
                env.push((os(name), None));
            }
            env.push((os("NODE_USE_ENV_PROXY"), Some(os("1"))));
        }
        args.push(os("--"));
        args.push(os(&program));
        args.extend(line.args);
        env.push((os("PATH"), Some(self.path.clone())));
        env.push((os("TMPDIR"), Some(os("/tmp"))));
        Ok(CommandLine {
            program: bwrap.to_path_buf(),
            args,
            env,
            cwd,
        })
    }

    /// What to bind so that `program` runs: its folder when it is installed
    /// under the home folder in a folder of its own (`claude`'s package or
    /// versions folder), else the file alone.
    fn program_root(&self, program: &Path) -> PathBuf {
        let own_folder = program.parent().filter(|parent| {
            parent
                .strip_prefix(&self.home)
                .is_ok_and(|rest| rest.components().count() >= 3)
                && !parent.ends_with("bin")
                && !NEVER_BOUND
                    .iter()
                    .any(|n| parent.starts_with(self.home.join(n)))
        });
        own_folder.map_or_else(|| program.to_path_buf(), Path::to_path_buf)
    }
}

/// Fails if a mount covers one made before it (binds its folder or one
/// above) and that one is not made again after: what it hid or protected
/// would be lost. Plans are built so this never happens; the check keeps
/// it that way.
fn nothing_lost(mounts: &[Mount]) -> Result<()> {
    let path = |m: &Mount| match m {
        Mount::Ro(p) | Mount::Rw(p) | Mount::Tmpfs(p) | Mount::Null(p) => p.clone(),
    };
    for (i, earlier) in mounts.iter().enumerate() {
        let covered = mounts[i + 1..]
            .iter()
            .rposition(|later| path(earlier).starts_with(path(later)) && later != earlier);
        if let Some(j) = covered
            && !mounts[i + 1 + j + 1..].contains(earlier)
        {
            return Err(ProtocolError::new(
                ErrorCode::Internal,
                format!("the sandbox would lose {}", path(earlier).display()),
            ));
        }
    }
    Ok(())
}

/// The folder name `claude` keeps a project's transcripts under: its path
/// with every character but letters and digits replaced by `-`.
fn project_key(work_dir: &Path) -> String {
    work_dir
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// `path` with every link resolved; it must exist.
fn real(path: &Path) -> Result<PathBuf> {
    fs::canonicalize(path).map_err(|e| invalid(format!("{}: {e}", path.display())))
}

/// A worker's own `<git dir>/worktrees/<name>`, found through its
/// worktree's `.git` file, if that folder is in a git folder the profile
/// lets it read and points back at this worktree. Anything else is not
/// trusted with write access.
fn worker_git_dir(work_dir: &Path, profile: &SandboxProfile) -> Result<Option<PathBuf>> {
    let dot_git = work_dir.join(".git");
    if !fs::symlink_metadata(&dot_git).is_ok_and(|m| m.is_file()) {
        return Ok(None);
    }
    let text = fs::read_to_string(&dot_git).unwrap_or_default();
    let Some(target) = text.strip_prefix("gitdir:").map(str::trim) else {
        return Ok(None);
    };
    let Ok(git_dir) = real(&work_dir.join(target)) else {
        return Ok(None);
    };
    let mine = profile
        .read
        .iter()
        .filter_map(|r| real(r).ok())
        .any(|common| {
            git_dir.parent() == Some(&common.join("worktrees"))
                && fs::read_to_string(git_dir.join("gitdir"))
                    .ok()
                    .and_then(|back| real(Path::new(back.trim())).ok())
                    == Some(dot_git.clone())
        });
    Ok(mine.then_some(git_dir))
}

/// Every protected path that exists. In a git folder under `work_dir` (a
/// direct session's `.git`, a nested repository's) all of it: git reads
/// `config`, `commondir` and hooks from many places in it, and the agent
/// has nothing to commit. A worker gets its own `config.worktree` made
/// empty, so it cannot make one. A protected path that is a link is
/// refused: the link's name is what claude and git look up, and it could
/// be replaced; binding its target would not stop that.
fn protected_paths(
    setup: &Setup,
    work_dir: &Path,
    worker: Option<&Path>,
    home: &Path,
    hidden: &[PathBuf],
) -> Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    let mut deep = vec![".git"];
    for pattern in setup.protected.work_dir {
        match pattern.strip_prefix("**/") {
            Some(name) => deep.push(name),
            None => found.push(work_dir.join(pattern)),
        }
    }
    found.push(work_dir.join(".git"));
    walk(work_dir, &deep, hidden, &mut found);
    if let Some(git_dir) = worker {
        for name in setup.protected.worktree_git_dir {
            let path = git_dir.join(name);
            if *name == "config.worktree" && !path.exists() {
                let _ = fs::File::create(&path);
            }
            found.push(path);
        }
    }
    if let Some(hooks) = hooks_path(work_dir, home) {
        found.push(hooks);
    }
    let mut protected = Vec::new();
    for path in found {
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.is_symlink() {
            return Err(invalid(format!(
                "{} is a link: the sandbox cannot keep it read-only. Replace it with the file itself to run agents here",
                path.display()
            )));
        }
        protected.push(real(&path)?);
    }
    protected.sort();
    protected.dedup();
    Ok(protected)
}

/// The folder `core.hooksPath` names, if set and inside `work_dir`; made
/// if missing, so the agent cannot make it. Elsewhere it is read-only
/// already.
fn hooks_path(work_dir: &Path, home: &Path) -> Option<PathBuf> {
    let out = Command::new("git")
        .args(["config", "--get", "core.hooksPath"])
        .current_dir(work_dir)
        .env_remove("GIT_DIR")
        .env_remove("GIT_CONFIG")
        .env_remove("GIT_CONFIG_GLOBAL")
        .env_remove("GIT_WORK_TREE")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8(out.stdout).ok()?;
    let text = text.trim();
    let path = match text.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => work_dir.join(text),
    };
    let rest = path.strip_prefix(work_dir).ok()?;
    if text.is_empty()
        || rest.as_os_str().is_empty()
        || !rest
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
    {
        return None;
    }
    // Made only through real folders inside `work_dir`.
    let mut at = work_dir.to_path_buf();
    for part in rest.components() {
        at.push(part);
        match fs::symlink_metadata(&at) {
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => return Some(at),
            Err(_) => {
                fs::create_dir(&at).ok()?;
            }
        }
    }
    Some(path)
}

/// Adds every entry under `dir` named one of `names`, without following
/// links or entering `.git` or the hidden folders.
fn walk(dir: &Path, names: &[&str], hidden: &[PathBuf], found: &mut Vec<PathBuf>) {
    if names.is_empty() {
        return;
    }
    let mut stack = vec![dir.to_path_buf()];
    let mut seen = 0;
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            seen += 1;
            if seen > WALK_LIMIT {
                return;
            }
            let name = entry.file_name();
            if names.iter().any(|n| name == **n) {
                found.push(entry.path());
            }
            let path = entry.path();
            if name != ".git"
                && entry.file_type().is_ok_and(|t| t.is_dir())
                && !hidden.contains(&path)
            {
                stack.push(path);
            }
        }
    }
}

#[cfg(test)]
mod tests;
