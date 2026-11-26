//! What one session's sandbox shows: the mounts bubblewrap makes, in order,
//! and what Landlock lets the helper write. Later mounts cover earlier
//! ones, so the order is: hide, bind back, then the protected paths
//! read-only on top.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use ovra_core::protocol::projects::ProjectId;
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{
    CommandLine, ProfileKind, ProfileNetwork, Result, SandboxProfile, WorkEnv,
};
use sha2::{Digest as _, Sha256};

use crate::helper::Landlock;
use crate::helper::OUTER_FLAG;
use crate::{HELPER_FLAG, Setup, invalid, os};

mod server;

/// The protected paths of `ovra-sandbox`, handed over by it (it depends on
/// this crate, not the other way round).
#[derive(Debug, Clone, Copy)]
pub struct Protected {
    /// Under `work_dir`; `**/name` is any depth.
    pub work_dir: &'static [&'static str],
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

/// The variables a session keeps from Ovra's environment; every other one
/// is removed, so tokens such as `GITHUB_TOKEN`, `GH_TOKEN`, `NPM_TOKEN`,
/// `AWS_*` credentials and `SSH_AUTH_SOCK` stay out (owner approved,
/// amending ADR 0006). Whole names only: a prefix such as `CLAUDE_CODE_`
/// would let `CLAUDE_CODE_OAUTH_TOKEN` through. The proxy variables,
/// `PATH` and `TMPDIR` are set by the plan itself.
const ENV_KEPT: [&str; 36] = [
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "LANG",
    "LANGUAGE",
    "LC_ALL",
    "LC_CTYPE",
    "TZ",
    "TERM",
    "COLORTERM",
    "NO_COLOR",
    // Where usage is billed, and the gateway it goes through.
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_MODEL",
    "ANTHROPIC_SMALL_FAST_MODEL",
    // Cloud providers: which one, its region and project, never its keys.
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_USE_FOUNDRY",
    "ANTHROPIC_BEDROCK_BASE_URL",
    "ANTHROPIC_VERTEX_BASE_URL",
    "ANTHROPIC_VERTEX_PROJECT_ID",
    "ANTHROPIC_FOUNDRY_RESOURCE",
    "ANTHROPIC_FOUNDRY_BASE_URL",
    "AWS_REGION",
    "CLOUD_ML_REGION",
    "GOOGLE_CLOUD_PROJECT",
    // Certificate bundles, for networks that inspect TLS.
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "NODE_EXTRA_CA_CERTS",
    // Toolchain homes.
    "CARGO_HOME",
    "RUSTUP_HOME",
    "NVM_DIR",
    "GOPATH",
    // The recording Ovra's own stand-in for `claude` plays in e2e and UX
    // runs: a file path, never a secret.
    "OVRA_FAKE_CLAUDE_FIXTURE",
];

/// Where TLS clients look for a CA bundle: OpenSSL and rustls, Node,
/// Python requests, curl, git.
const CA_BUNDLE_VARS: [&str; 5] = [
    "SSL_CERT_FILE",
    "NODE_EXTRA_CA_CERTS",
    "REQUESTS_CA_BUNDLE",
    "CURL_CA_BUNDLE",
    "GIT_SSL_CAINFO",
];

/// Protected folders made empty in the session's folder when missing, so
/// the agent cannot create them (`.claude/settings.json` hooks, other
/// agents' MCP servers and hooks, editor tasks, `.ovra/servers.json`
/// commands) for the user's own tools to run later. Those an agent's
/// session made are removed again once the last sandbox of the work dir
/// has ended ([`crate::made`]); those a pod made stay.
const FOLDERS_MADE: [&str; 10] = [
    ".claude",
    ".codex",
    ".gemini",
    ".cursor",
    ".opencode",
    ".vscode",
    ".idea",
    ".zed",
    ".devcontainer",
    ".ovra",
];

/// Protected files made empty at the top of the session's folder when
/// missing, for the same reason: agent instructions, MCP servers and
/// `direnv` scripts the user's own tools would load later, and the list of
/// ignored files Ovra copies into the project's next worktrees
/// (`.worktreeinclude`). Empty files show
/// in git status, so whoever made them (agents and pods alike) removes
/// them again once the last sandbox of the work dir has ended, if they are
/// still empty. One in a folder below stays to the review.
const FILES_MADE: [&str; 6] = [
    "CLAUDE.md",
    "CLAUDE.local.md",
    "AGENTS.md",
    ".mcp.json",
    ".envrc",
    ".worktreeinclude",
];

/// Which of [`FILES_MADE`] the protected list names at the top of the
/// work dir, as `name` or `**/name`.
fn files_made(protected: &[&str]) -> impl Iterator<Item = &'static str> {
    FILES_MADE.into_iter().filter(|name| {
        protected
            .iter()
            .any(|p| p == name || p.strip_prefix("**/") == Some(name))
    })
}

/// Package caches builds write, relative to the home folder. Each session
/// gets its project's own copy (empty at first), shared by all the
/// project's work dirs ([`cache_key`]), bound where the cache is if that
/// cache or its parent exists: one shared with the user's own builds would
/// let an agent plant a package they later run.
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

/// Folders that may show the home folder or other disks by another path.
const OTHER_ROUTES: [&str; 4] = ["/mnt", "/media", "/sysroot", "/.snapshots"];

/// The port inside the sandbox where the helper bridges the netproxy.
pub(crate) const PROXY_PORT: u16 = 3128;

/// A walk for `**/` names stops after this many entries.
const WALK_LIMIT: usize = 500_000;

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
    /// A folder of Ovra's, bound writable at `at`.
    Private { from: PathBuf, at: PathBuf },
}

/// The sandbox of one session.
#[derive(Debug)]
pub(crate) struct Plan {
    home: PathBuf,
    mounts: Vec<Mount>,
    /// The folders and files Landlock lets the agent write.
    writable: Vec<PathBuf>,
    network: ProfileNetwork,
    /// Ports the helper listens on inside the sandbox and forwards to the
    /// netproxy with `OVRA-LOCAL <port>`.
    forward: Vec<u16>,
    path: OsString,
    /// Folders of [`FOLDERS_MADE`] an agent's plan made, and files of
    /// [`FILES_MADE`] any plan made: removed again once every sandbox of
    /// the work dir and everything they started are gone.
    temporary: Vec<PathBuf>,
    /// The work dir's record of those folders, held shared while the
    /// sandbox runs (agents and pods; [`crate::made`]).
    record: Option<PathBuf>,
    kind: Kind,
    /// The account's configuration folder, resolved: `claude` is pointed
    /// at this path, which the sandbox shows even when the one it was given
    /// goes through a link in the hidden home folder.
    claude_dir: Option<PathBuf>,
    /// The work dir's variables, set on the sandbox's own line.
    work_env: Option<WorkEnv>,
}

/// Who the sandbox is for.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    /// An agent session: [`ENV_KEPT`], and the working folder its line names.
    Agent,
    /// A server pod (a profile that exposes ports): an agent's sandbox
    /// without `claude`'s state (all of `~/.claude` and `~/.claude.json`
    /// stay hidden), with [`server::SERVER_ENV_KEPT`]. Servers run the
    /// project's code, not `claude`: they get no login, no transcripts and
    /// no Claude or cloud-provider variables.
    Pod,
    /// The agent browser: a pod's sandbox without the home folder's tools,
    /// git settings and caches; Chromium needs only its profile folder.
    /// It runs web pages with Chromium's own sandbox off, so it gets
    /// nothing of the person's that it does not need.
    Browser,
    /// A language server: [`server::SERVER_ENV_KEPT`], the line's own
    /// variables over the plan's, and the project as its working folder.
    Server { project: PathBuf },
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
        let own = match (&profile.claude_dir, profile.kind) {
            (Some(dir), ProfileKind::Agent) => Some(account_dir(dir, &home, &work_dir, setup)?),
            _ => None,
        };
        // `claude`'s own configuration stays read-only, whatever the project
        // and the account: the default account's and every other one's.
        for path in std::iter::once(&work_dir).chain(&profile.write) {
            let path = real(path).unwrap_or_else(|_| path.clone());
            if holds_claude_state(&path, &home) {
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
            forward: ports(profile)?,
            path: OsString::new(),
            temporary: Vec::new(),
            record: None,
            claude_dir: own.clone(),
            work_env: profile.work_env.clone(),
            kind: match profile.kind {
                ProfileKind::Agent => Kind::Agent,
                ProfileKind::Pod => Kind::Pod,
                ProfileKind::Browser => Kind::Browser,
            },
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
        let caches = cache_key(profile.project.as_ref(), &work_dir);
        plan.home_back(setup, (&home, &work_dir), &caches, &shown, own.as_deref());
        for read in &profile.read {
            if let Ok(read) = real(read) {
                plan.mounts.push(Mount::Ro(read));
            }
        }
        for write in &profile.write {
            plan.rw(real(write)?);
        }
        let read_only = profile.work_dir_read_only;
        if read_only {
            plan.mounts.push(Mount::Ro(work_dir.clone()));
        } else {
            plan.rw(work_dir.clone());
        }
        let mut hidden: Vec<PathBuf> = setup.hidden.iter().filter_map(|h| real(h).ok()).collect();
        hidden.sort_by_key(|h| h.components().count());
        // Held while folders are made and recorded, so no cleaner of an
        // ended sandbox removes them meanwhile.
        let record = crate::made::record_of(&setup.data_dir, &project_key(&work_dir));
        let held = crate::made::hold(&record).map_err(|e| {
            invalid(format!(
                "cannot keep track of the folders made for the sandbox in {}: {e}",
                record.display()
            ))
        })?;
        // A read-only work dir needs no protected paths: nothing in it is
        // writable, so nothing is made there either.
        let absent = |name: &str| !read_only && fs::symlink_metadata(work_dir.join(name)).is_err();
        let missing: Vec<PathBuf> = FOLDERS_MADE
            .into_iter()
            .filter(|name| absent(name))
            .map(|name| work_dir.join(name))
            .collect();
        let missing_files: Vec<PathBuf> = files_made(setup.protected.work_dir)
            .filter(|name| absent(name))
            .map(|name| work_dir.join(name))
            .collect();
        // Skipped with its refusal of protected names that are links: a
        // link in a read-only bind leads nowhere writable that the plan
        // does not already allow.
        let protected = if read_only {
            Vec::new()
        } else {
            protected_paths(setup, &work_dir, &home, &hidden)?
        };
        // A pod's folders stay: removing them could drop the bind under an
        // agent's session that started meanwhile.
        if plan.kind == Kind::Agent {
            plan.temporary = missing
                .into_iter()
                .filter(|path| fs::symlink_metadata(path).is_ok_and(|m| m.is_dir()))
                .collect();
        }
        plan.temporary.extend(
            missing_files.into_iter().filter(|path| {
                fs::symlink_metadata(path).is_ok_and(|m| m.is_file() && m.len() == 0)
            }),
        );
        crate::made::add(&held, &plan.temporary)
            .map_err(|e| invalid(format!("cannot record {}: {e}", record.display())))?;
        drop(held);
        plan.record = Some(record);
        plan.protect(protected);
        let before = plan.mounts.len();
        // Ovra's data folder inside a folder bound above (a project that
        // holds `.ovra-data`) is hidden again.
        for hidden in hidden {
            let covered = plan.mounts[before..]
                .iter()
                .any(|m| matches!(m, Mount::Tmpfs(t) if hidden.starts_with(t)));
            let shown = plan.under_writable(&hidden).is_some()
                || (read_only && hidden.starts_with(&work_dir));
            if !work_dir.starts_with(&hidden) && shown && !covered {
                plan.mounts.push(Mount::Tmpfs(hidden));
            }
        }
        if let ProfileNetwork::Proxy { address, ca_bundle } = &profile.network {
            plan.mounts.push(Mount::Ro(real(Path::new(address))?));
            if let Some(bundle) = ca_bundle {
                plan.mounts.push(Mount::Ro(real(bundle)?));
            }
        }
        Ok(plan)
    }

    /// Binds `path` read-only unless a folder bound before already shows
    /// it: binding it again would cover what was bound inside it since.
    fn ro(&mut self, path: PathBuf) {
        let shown = self.mounts.iter().any(|m| match m {
            Mount::Ro(p) | Mount::Rw(p) | Mount::Private { at: p, .. } => path.starts_with(p),
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
        self.mounts.push(Mount::Tmpfs("/var/tmp".into()));
        self.mounts.push(Mount::Tmpfs("/run".into()));
        // Other ways to the same files: other disks, snapshots, the
        // system's own root on ostree.
        for other in OTHER_ROUTES {
            if Path::new(other).is_dir() && !home.starts_with(other) {
                self.mounts.push(Mount::Tmpfs(other.into()));
            }
        }
        if let Ok(resolv) = fs::canonicalize("/etc/resolv.conf")
            && resolv.starts_with("/run")
        {
            self.mounts.push(Mount::Ro(resolv));
        }
        self.mounts.push(Mount::Tmpfs(home.to_path_buf()));
        // Outer folders first; one inside a folder hidden already is hidden
        // with it (hiding it again would be covered by the outer one).
        let mut hidden: Vec<PathBuf> = setup.hidden.iter().filter_map(|h| real(h).ok()).collect();
        hidden.sort_by_key(|h| h.components().count());
        for hidden in hidden {
            let covered = self
                .mounts
                .iter()
                .any(|m| matches!(m, Mount::Tmpfs(t) if hidden.starts_with(t)));
            if hidden.is_dir() && !covered {
                self.mounts.push(Mount::Tmpfs(hidden));
            }
        }
    }

    /// What `claude`, git and the toolchains need from the home folder.
    /// `caches`: the folder of the project's package caches ([`cache_key`]).
    /// `own`: the account's configuration folder, if it has one.
    fn home_back(
        &mut self,
        setup: &Setup,
        (home, work_dir): (&Path, &Path),
        caches: &str,
        shown: &[PathBuf],
        own: Option<&Path>,
    ) {
        if self.kind == Kind::Agent {
            self.claude_back(home, work_dir, own);
        }
        if self.kind == Kind::Browser {
            // The system's programs only; an empty `PATH` would search
            // the working folder, which the browser writes.
            self.path = "/usr/bin:/bin".into();
            return;
        }
        self.home_read(setup, home, shown);
        self.cover_secrets(home);
        self.private_caches(setup, home, caches);
    }

    /// `claude`'s own state: its settings read-only, what it writes during
    /// a session private or writable. With an account's own folder `own`,
    /// that folder and the state file in it instead of `~/.claude` and
    /// `~/.claude.json`, which stay hidden with every other account's.
    fn claude_back(&mut self, home: &Path, work_dir: &Path, own: Option<&Path>) {
        let (claude, state) = claude_state(home, own);
        if own.is_none() {
            let _ = fs::create_dir_all(&claude);
        }
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
                // Its memory loads into every later session there, the
                // user's own too: read-only, like `CLAUDE.md`.
                let memory = transcripts.join("memory");
                self.rw(transcripts.clone());
                if fs::create_dir_all(&memory).is_ok()
                    && let Ok(memory) = real(&memory)
                    && memory.starts_with(&transcripts)
                {
                    self.mounts.push(Mount::Ro(memory));
                }
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
        if fs::symlink_metadata(&state).is_ok_and(|m| m.is_file()) {
            self.rw(state);
        }
    }

    /// What git and the toolchains read from the home folder, and the
    /// toolchains on `PATH`.
    fn home_read(&mut self, setup: &Setup, home: &Path, shown: &[PathBuf]) {
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
    }

    /// `/dev/null` over [`SECRETS`] in the home folder's bound folders. Comes
    /// after them, so nothing covers it again.
    fn cover_secrets(&mut self, home: &Path) {
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
    }

    /// The project's own copy of each package cache in [`CACHES`], bound
    /// where the user's is.
    fn private_caches(&mut self, setup: &Setup, home: &Path, key: &str) {
        let caches = setup.caches.join(key);
        for name in CACHES {
            let cache = home.join(name);
            // `~/.npm` only if it exists; `~/.cargo/registry` if `~/.cargo` does.
            let parent = cache.parent().filter(|p| *p != home && p.is_dir());
            let from = caches.join(project_key(Path::new(name)));
            if (cache.is_dir() || parent.is_some())
                && fs::create_dir_all(&cache).is_ok()
                && let Ok(cache) = real(&cache)
                && cache.starts_with(home)
                && fs::create_dir_all(&from).is_ok()
                && let Ok(from) = real(&from)
            {
                self.writable.push(cache.clone());
                self.mounts.push(Mount::Private { from, at: cache });
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
                Mount::Rw(root) | Mount::Private { at: root, .. } if path.starts_with(root) => {
                    Some(root.clone())
                }
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
        outer_env: &[OsString],
    ) -> Result<CommandLine> {
        let program = real(&line.program)?;
        let helper = real(helper)?;
        let cwd = match &self.kind {
            Kind::Agent | Kind::Pod | Kind::Browser => real(&line.cwd)?,
            Kind::Server { project } => project.clone(),
        };
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
        // The helper and the program, wherever they are installed. A
        // program's own folder may hold the helper already (a build next
        // to it): binding the helper first would be lost under it.
        let program_root = self.program_root(&program);
        if helper == program_root || !helper.starts_with(&program_root) {
            mounts.push(Mount::Ro(helper.clone()));
        }
        mounts.push(Mount::Ro(program_root));
        nothing_lost(&mounts)?;
        for mount in &mounts {
            let (flag, from, to) = match mount {
                Mount::Ro(p) => ("--ro-bind", p.as_os_str(), p),
                Mount::Rw(p) => ("--bind", p.as_os_str(), p),
                Mount::Null(p) => ("--ro-bind", "/dev/null".as_ref(), p),
                Mount::Private { from, at } => ("--bind", from.as_os_str(), at),
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
        if matches!(self.kind, Kind::Server { .. }) {
            args.push(os("--no-unix-sockets"));
        }
        // A server's line points its caches and temporary files into its
        // cache folder: its variables come last, so they win.
        let (mut env, last) = match self.kind {
            Kind::Agent | Kind::Pod | Kind::Browser => (line.env, Vec::new()),
            Kind::Server { .. } => (Vec::new(), line.env),
        };
        if let ProfileNetwork::Proxy { address, ca_bundle } = &self.network {
            args.extend([os("--bridge"), os(real(Path::new(address))?)]);
            args.push(os(PROXY_PORT.to_string()));
            for port in &self.forward {
                args.extend([os("--forward"), os(port.to_string())]);
            }
            let proxy = os(format!("http://127.0.0.1:{PROXY_PORT}"));
            for name in ["HTTP_PROXY", "HTTPS_PROXY", "http_proxy", "https_proxy"] {
                env.push((os(name), Some(proxy.clone())));
            }
            for name in ["NO_PROXY", "no_proxy", "ALL_PROXY", "all_proxy"] {
                env.push((os(name), None));
            }
            env.push((os("NODE_USE_ENV_PROXY"), Some(os("1"))));
            // The proxy adds secrets under its own CA: every TLS client
            // trusts the bundle that holds it, and no folder of roots that
            // would be read instead.
            if let Some(bundle) = ca_bundle {
                let bundle = os(real(bundle)?);
                for name in CA_BUNDLE_VARS {
                    env.push((os(name), Some(bundle.clone())));
                }
                env.push((os("SSL_CERT_DIR"), None));
            }
        }
        env.push((os("PATH"), Some(self.path.clone())));
        env.push((os("TMPDIR"), Some(os("/tmp"))));
        if let Some(dir) = &self.claude_dir {
            env.push((os("CLAUDE_CONFIG_DIR"), Some(os(dir))));
        }
        if let Some(work) = &self.work_env {
            env.push((os("OVRA_PORT"), Some(os(work.port.to_string()))));
            env.push((os("OVRA_WORKTREE_ROOT"), Some(os(&work.worktree_root))));
            env.push((os("OVRA_MAIN_ROOT"), Some(os(&work.main_root))));
        }
        env.extend(last);
        let names = self.env_kept();
        allowlist(outer_env, names, &mut env);
        // The helper keeps only these: the plan removes the rest, and
        // whatever Ovra's environment gains later.
        let mut kept: Vec<OsString> = names.iter().map(os).collect();
        for (name, value) in &env {
            if value.is_some() && !kept.contains(name) {
                kept.push(name.clone());
            }
        }
        for name in kept.into_iter().filter(|name| !removed(&env, name)) {
            args.extend([os("--env"), name]);
        }
        args.push(os("--"));
        args.push(os(&program));
        args.extend(line.args);
        let Some(record) = &self.record else {
            return Ok(CommandLine {
                program: bwrap.to_path_buf(),
                args,
                env,
                cwd,
            });
        };
        // The helper outside the sandbox holds the work dir's record while
        // the session runs, and cleans up once it has ended, however it ends.
        let mut outer = vec![os(OUTER_FLAG), os(record)];
        outer.extend([os("--"), os(bwrap)]);
        outer.extend(args);
        Ok(CommandLine {
            program: helper,
            args: outer,
            env,
            cwd,
        })
    }

    /// The variables this sandbox keeps from Ovra's environment.
    fn env_kept(&self) -> &'static [&'static str] {
        match self.kind {
            Kind::Agent => &ENV_KEPT,
            Kind::Pod | Kind::Browser | Kind::Server { .. } => &server::SERVER_ENV_KEPT,
        }
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

/// Checks the ports of a profile and returns those to forward, sorted.
/// A server pod (`expose`) needs a network namespace of its own, or its
/// servers would listen on the host; forwarding needs the netproxy, which
/// applies the loopback rule. No port may be the proxy's own inside the
/// sandbox, and none both exposed and forwarded: each is one listener
/// there. Apart from `claude`'s state and the environment ([`Kind::Pod`]),
/// a pod is an agent's sandbox: its own PID namespace and
/// `--die-with-parent` are on for every session, and
/// Landlock and seccomp already let it listen on TCP inside its network
/// namespace and make Unix sockets in its writable folders.
fn ports(profile: &SandboxProfile) -> Result<Vec<u16>> {
    let proxy = matches!(profile.network, ProfileNetwork::Proxy { .. });
    if !profile.expose.is_empty() && profile.kind != ProfileKind::Pod {
        return Err(invalid("only a server pod exposes ports"));
    }
    if !profile.expose.is_empty() && matches!(profile.network, ProfileNetwork::Open) {
        return Err(invalid(
            "a server pod needs a network of its own: the sandbox cannot run one with an open network",
        ));
    }
    if !profile.forward.is_empty() && !proxy {
        return Err(invalid(
            "forwarding ports to servers needs the session's netproxy",
        ));
    }
    for &port in profile.expose.iter().chain(&profile.forward) {
        if port == 0 || (proxy && port == PROXY_PORT) {
            return Err(invalid(format!(
                "port {port} cannot be used by a server: the sandbox needs it"
            )));
        }
    }
    if let Some(port) = profile.forward.iter().find(|p| profile.expose.contains(p)) {
        return Err(invalid(format!(
            "port {port} cannot be both exposed and forwarded"
        )));
    }
    let mut forward = profile.forward.clone();
    forward.sort_unstable();
    forward.dedup();
    Ok(forward)
}

/// Fails if a mount covers one made before it (binds its folder or one
/// above) and that one is not made again after: what it hid or protected
/// would be lost. Plans are built so this never happens; the check keeps
/// it that way.
fn nothing_lost(mounts: &[Mount]) -> Result<()> {
    let path = |m: &Mount| match m {
        Mount::Ro(p)
        | Mount::Rw(p)
        | Mount::Tmpfs(p)
        | Mount::Null(p)
        | Mount::Private { at: p, .. } => p.clone(),
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

/// The folder name of a project's package caches in `Setup::caches`: a
/// hash of its id, so every work dir of the project (its main checkout and
/// its worktrees) shares one and no two projects do. A profile that names
/// no project gets one of its work dir's own.
fn cache_key(project: Option<&ProjectId>, work_dir: &Path) -> String {
    let (kind, name) = match project {
        Some(id) => ("project", id.0.as_bytes()),
        None => ("folder", work_dir.as_os_str().as_encoded_bytes()),
    };
    let digest = Sha256::new()
        .chain_update(kind)
        .chain_update([0])
        .chain_update(name)
        .finalize();
    let hex: String = digest[..16].iter().map(|b| format!("{b:02x}")).collect();
    format!("{}-{hex}", &kind[..1])
}

/// `claude`'s configuration folder and state file: an account's own folder
/// keeps its state file inside (`CLAUDE_CONFIG_DIR`), the default account
/// uses `~/.claude` and `~/.claude.json`.
fn claude_state(home: &Path, own: Option<&Path>) -> (PathBuf, PathBuf) {
    match own {
        Some(dir) => (dir.to_path_buf(), dir.join(".claude.json")),
        None => (home.join(".claude"), home.join(".claude.json")),
    }
}

/// Whether `name`, right in the home folder, holds `claude`'s state:
/// `.claude`, `.claude.json` or an account's folder such as `.claude-work`.
fn claude_name(name: &std::ffi::OsStr) -> bool {
    name.to_string_lossy().starts_with(".claude")
}

/// Whether `path` (resolved) is in or is one of the home folder's
/// [`claude_name`] entries, which every account's sessions load from.
fn holds_claude_state(path: &Path, home: &Path) -> bool {
    path.strip_prefix(home)
        .ok()
        .and_then(|rest| rest.components().next())
        .is_some_and(|top| claude_name(top.as_os_str()))
}

/// The account's configuration folder, resolved, if the sandbox may give
/// it back: an existing folder right in the home folder whose name starts
/// with `.claude` (not `.claude` itself), like the ones Ovra suggests, and
/// that holds none of Ovra's hidden folders. Any other folder could hold
/// the person's secrets or a project an agent writes.
fn account_dir(dir: &Path, home: &Path, work_dir: &Path, setup: &Setup) -> Result<PathBuf> {
    let refused = |why: &str| {
        Err(invalid(format!(
            "the sandbox does not use {} as a Claude account's folder: {why}",
            dir.display()
        )))
    };
    let Ok(real_dir) = real(dir) else {
        return refused("it does not exist; sign in to that account once to create it");
    };
    if !real_dir.is_dir() {
        return refused("it is not a folder");
    }
    let named = real_dir
        .file_name()
        .is_some_and(|n| claude_name(n) && n != ".claude");
    if real_dir.parent() != Some(home) || !named {
        return refused(
            "it must be a folder right in the home folder whose name starts with .claude, such as ~/.claude-work",
        );
    }
    if real_dir.starts_with(work_dir) || work_dir.starts_with(&real_dir) {
        return refused("it overlaps the session's folder");
    }
    let hides = setup
        .hidden
        .iter()
        .filter_map(|h| real(h).ok())
        .any(|h| h.starts_with(&real_dir) || real_dir.starts_with(&h));
    if hides {
        return refused("it holds Ovra's own data");
    }
    Ok(real_dir)
}

/// `path` with every link resolved; it must exist.
fn real(path: &Path) -> Result<PathBuf> {
    fs::canonicalize(path).map_err(|e| invalid(format!("{}: {e}", path.display())))
}

/// Every protected path that exists. In a git folder under `work_dir` (a
/// direct session's `.git`, a nested repository's) all of it: git reads
/// `config`, `commondir` and hooks from many places in it, and the agent
/// has nothing to commit. A worker's own git folder stays read-only with
/// the rest of the main repository's (`read`): with its index the agent
/// could add a submodule whose filters Ovra's git would run. Submodule
/// folders that are not checked out stay read-only, so the agent cannot
/// make a repository there that git steps into; so do config files inside
/// `work_dir` that git includes. A protected path that is a link is
/// refused: the link's name is what claude and git look up, and it could
/// be replaced; binding its target would not stop that.
fn protected_paths(
    setup: &Setup,
    work_dir: &Path,
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
    walk(work_dir, &deep, hidden, &mut found)?;
    let mut made: Vec<(PathBuf, bool)> = FOLDERS_MADE
        .iter()
        .filter(|name| setup.protected.work_dir.contains(name))
        .map(|name| (work_dir.join(name), true))
        .collect();
    made.extend(files_made(setup.protected.work_dir).map(|name| (work_dir.join(name), false)));
    if let Some(hooks) = hooks_path(work_dir, home) {
        made.push((hooks, true));
    }
    made.extend(config_files(work_dir, home).into_iter().map(|f| (f, false)));
    made.extend(empty_submodules(work_dir).into_iter().map(|d| (d, true)));
    for (path, folder) in made {
        found.extend(made_inside(work_dir, &path, folder)?);
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
    // Sorted, a folder comes before what is in it, which it covers anyway.
    protected.sort();
    let mut kept: Vec<PathBuf> = Vec::new();
    for path in protected {
        if !kept.iter().any(|k| path.starts_with(k)) {
            kept.push(path);
        }
    }
    Ok(kept)
}

/// What `git <args>` prints in `work_dir`, as the user's git sees it;
/// `None` outside a repository or without git.
fn git(work_dir: &Path, args: &[&str]) -> Option<String> {
    // Never anything from the repository's config that runs a program.
    let out = Command::new("git")
        .args(["-c", "core.fsmonitor=false", "--no-optional-locks"])
        .args(args)
        .current_dir(work_dir)
        .env_remove("GIT_DIR")
        .env_remove("GIT_CONFIG")
        .env_remove("GIT_CONFIG_GLOBAL")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    String::from_utf8(out.stdout).ok()
}

/// Config files git reads for `work_dir` that lie inside it: those that
/// set something, and those an `include.path` or `includeIf.*.path` names
/// (an empty one could be filled).
fn config_files(work_dir: &Path, home: &Path) -> Vec<PathBuf> {
    let listed = git(
        work_dir,
        &[
            "config",
            "--list",
            "--show-origin",
            "--includes",
            "--name-only",
            "-z",
        ],
    )
    .unwrap_or_default();
    let mut files: Vec<PathBuf> = listed
        .split('\0')
        .filter_map(|field| field.strip_prefix("file:"))
        .map(|file| work_dir.join(file))
        .collect();
    let includes = git(
        work_dir,
        &[
            "config",
            "--show-origin",
            "--includes",
            "-z",
            "--get-regexp",
            r"^include(if\..*)?\.path$",
        ],
    )
    .unwrap_or_default();
    let fields: Vec<&str> = includes.split('\0').collect();
    for pair in fields.chunks(2) {
        let [origin, entry] = pair else { continue };
        let (Some(origin), Some((_, target))) =
            (origin.strip_prefix("file:"), entry.split_once('\n'))
        else {
            continue;
        };
        let target = match target.strip_prefix("~/") {
            Some(rest) => home.join(rest),
            None => work_dir
                .join(origin)
                .parent()
                .map_or_else(PathBuf::new, |dir| dir.join(target)),
        };
        files.push(target);
    }
    files
        .into_iter()
        .filter(|file| lexically_inside(file, work_dir))
        .collect()
}

/// Removes every variable of Ovra's environment (`outer_env`, its names)
/// that is neither in `names` nor set by `env`, so bubblewrap's own processes,
/// whose environment the agent may read, hold none of them.
fn allowlist(outer_env: &[OsString], names: &[&str], env: &mut Vec<(OsString, Option<OsString>)>) {
    for name in outer_env {
        let kept = names.iter().any(|k| name == k);
        if !kept && !env.iter().any(|(n, _)| n == name) {
            env.push((name.clone(), None));
        }
    }
}

/// Whether the last word on `name` in `env` removes it.
fn removed(env: &[(OsString, Option<OsString>)], name: &OsString) -> bool {
    env.iter()
        .rev()
        .find(|(n, _)| n == name)
        .is_some_and(|(_, value)| value.is_none())
}

/// Whether `path`, with `..` resolved by name, is inside `dir`.
fn lexically_inside(path: &Path, dir: &Path) -> bool {
    let mut normal = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::ParentDir => {
                normal.pop();
            }
            std::path::Component::CurDir => {}
            other => normal.push(other),
        }
    }
    normal.starts_with(dir) && normal != dir
}

/// Submodule folders (gitlinks in the index) that are not checked out,
/// looking inside every submodule that is: `git status` steps into those
/// whether or not they are active, which `ls-files --recurse-submodules`
/// does not follow. Only gitlinks are followed, from `work_dir` down, and
/// only through real folders inside it: a repository the agent made is
/// never asked, as its config could run code.
fn empty_submodules(work_dir: &Path) -> Vec<PathBuf> {
    let mut empty = Vec::new();
    let mut seen = std::collections::HashSet::from([work_dir.to_path_buf()]);
    let mut repos = vec![work_dir.to_path_buf()];
    while let Some(repo) = repos.pop() {
        let text = git(&repo, &["ls-files", "--stage", "-z"]).unwrap_or_default();
        for record in text.split('\0') {
            let Some((_, path)) = record
                .strip_prefix("160000 ")
                .and_then(|r| r.split_once('\t'))
            else {
                continue;
            };
            // Outside `work_dir` it is read-only already.
            let Some(parts) = parts_inside(work_dir, &repo.join(path)) else {
                continue;
            };
            let dir = parts.iter().fold(work_dir.to_path_buf(), |d, p| d.join(p));
            let real = parts.iter().scan(work_dir.to_path_buf(), |at, part| {
                at.push(part);
                Some(fs::symlink_metadata(&*at).is_ok_and(|m| m.is_dir()))
            });
            let checked_out = real.into_iter().all(|is_dir| is_dir)
                && fs::symlink_metadata(dir.join(".git")).is_ok();
            if !checked_out {
                empty.push(dir);
            } else if seen.insert(dir.clone()) {
                repos.push(dir);
            }
        }
    }
    empty
}

/// The parts of `path` below `work_dir`, with `.` and `..` resolved by
/// name; `None` when it is not below it. An absolute path that reaches
/// `work_dir` through a link (`/home` for `/var/home`) counts too.
fn parts_inside(work_dir: &Path, path: &Path) -> Option<Vec<std::ffi::OsString>> {
    use std::path::Component;
    let rest = match path.strip_prefix(work_dir) {
        Ok(rest) => rest.to_path_buf(),
        Err(_) => {
            let (ancestor, real) = path
                .ancestors()
                .find_map(|a| fs::canonicalize(a).ok().map(|r| (a, r)))?;
            real.join(path.strip_prefix(ancestor).ok()?)
                .strip_prefix(work_dir)
                .ok()?
                .to_path_buf()
        }
    };
    let mut parts = Vec::new();
    for part in rest.components() {
        match part {
            Component::Normal(name) => parts.push(name.to_os_string()),
            Component::CurDir => {}
            Component::ParentDir => {
                parts.pop()?;
            }
            _ => return None,
        }
    }
    (!parts.is_empty()).then_some(parts)
}

/// The folder `core.hooksPath` names, if set and inside `work_dir`.
/// Elsewhere it is read-only already.
fn hooks_path(work_dir: &Path, home: &Path) -> Option<PathBuf> {
    let text = git(work_dir, &["config", "--get", "core.hooksPath"])?;
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    Some(match text.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => work_dir.join(text),
    })
}

/// What git would read inside `work_dir` must exist before the session
/// starts, or the agent could make it. This makes `path` (a folder, or an
/// empty file) if it is missing, only through real folders inside
/// `work_dir`, and returns what to bind: `path`, or the first part of it
/// that is not a folder. `None` when `path` is not inside `work_dir`.
fn made_inside(work_dir: &Path, path: &Path, folder: bool) -> Result<Option<PathBuf>> {
    use std::os::unix::fs::OpenOptionsExt as _;
    let Some(parts) = parts_inside(work_dir, path) else {
        return Ok(None);
    };
    let refuse = |at: &Path, error: std::io::Error| {
        invalid(format!(
            "cannot make {} for the sandbox to keep read-only: {error}",
            at.display()
        ))
    };
    let mut at = work_dir.to_path_buf();
    for (n, part) in parts.iter().enumerate() {
        at.push(part);
        let is_last = n + 1 == parts.len();
        match fs::symlink_metadata(&at) {
            Ok(meta) if meta.is_dir() && !is_last => {}
            Ok(_) => return Ok(Some(at)),
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err(refuse(&at, error));
            }
            Err(_) if is_last && !folder => {
                fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .custom_flags(libc::O_NOFOLLOW)
                    .open(&at)
                    .map_err(|e| refuse(&at, e))?;
            }
            Err(_) => fs::create_dir(&at).map_err(|e| refuse(&at, e))?,
        }
    }
    Ok(Some(at))
}

/// Adds every entry under `dir` named one of `names`, without following
/// links or entering `.git` or the hidden folders. A folder too large to
/// look through refuses the session: what the walk did not see would stay
/// writable.
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
                    "{} holds more than {WALK_LIMIT} files: the sandbox cannot check them for protected paths",
                    dir.display()
                )));
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
    Ok(())
}

#[cfg(test)]
mod tests;
