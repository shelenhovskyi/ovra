//! What one session's sandbox may read and write on macOS, planned from
//! paths alone so it is tested on every Unix; [`crate::sbpl`] writes it as
//! a Seatbelt profile.
//!
//! Seatbelt checks every access against the profile, whether the path
//! exists or not, so unlike on Linux nothing has to be made or found
//! before a session: a protected path stays read-only at any depth, also
//! one the agent tries to create. Its rules are allow rules on top of
//! "deny everything"; what a folder does not allow is written as a
//! condition of the allow rule itself ([`Plan::hidden`], [`Plan::never`],
//! [`Plan::protected`]), so no rule depends on the order of another.

use std::ffi::OsString;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

use ovra_core::protected::AGENT_STATE_IN_HOME;
use ovra_core::protocol::projects::ProjectId;
use ovra_core::services::{
    CommandLine, ProfileKind, ProfileNetwork, ProviderLists, Result, SandboxProfile, ServerProfile,
};
use sha2::{Digest as _, Sha256};

use crate::{HELPER_FLAG, Setup, invalid};

/// The variables every sandbox keeps from Ovra's environment, besides an
/// agent's own ([`ProviderLists::env`]), as on Linux: every other one is
/// removed, so `GITHUB_TOKEN`, `GH_TOKEN`, `NPM_TOKEN`, `AWS_*` and
/// `SSH_AUTH_SOCK` stay out. Whole names only. `PATH`, `TMPDIR`, the proxy
/// and cache variables are set by the plan or the helper.
const ENV_PERSON: [&str; 12] = [
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
];

/// See [`ENV_PERSON`].
const ENV_TOOLS: [&str; 7] = [
    // Certificate bundles, for networks that inspect TLS.
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "NODE_EXTRA_CA_CERTS",
    // Toolchain homes.
    "CARGO_HOME",
    "RUSTUP_HOME",
    "NVM_DIR",
    "GOPATH",
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

/// Package caches builds write: the variable that moves each one, and the
/// folder of the project's own copy. Without bind mounts the tools are
/// pointed at the copy, as on Windows; the person's own caches stay
/// unreadable to agents. `cargo` itself is still found on `PATH`.
const CACHES: [(&str, &str); 4] = [
    ("CARGO_HOME", "cargo"),
    ("npm_config_cache", "npm"),
    ("PIP_CACHE_DIR", "pip"),
    ("npm_config_store_dir", "pnpm"),
];

/// The person's own package caches, which a language server reads (never
/// writes) to resolve dependencies.
const USER_CACHES: [&str; 5] = [
    ".cargo/registry",
    ".cargo/git",
    ".npm",
    ".cache/pip",
    "Library/Caches/pip",
];

/// Files that stay readable from the home folder. Not all of
/// `~/.config/git`: git's credential store keeps `credentials` there.
const HOME_READ: [&str; 4] = [
    ".gitconfig",
    ".config/git/config",
    ".config/git/ignore",
    ".config/git/attributes",
];

/// Secrets inside readable folders of the home folder (a toolchain's).
const SECRETS: [&str; 3] = ["credentials", "credentials.toml", "credentials.json"];

/// Folders under the home folder whose `bin` on `PATH` brings the whole
/// folder (the toolchain's libraries live next to it). Any other `bin` is
/// readable alone: `~/.docker` or `~/.fly` keep tokens next to it.
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

/// Paths under the home folder nothing in the sandbox reads or writes,
/// whatever else gives them back.
const NEVER: [&str; 11] = [
    ".ssh",
    ".aws",
    ".gnupg",
    ".docker",
    ".kube",
    ".netrc",
    ".config/gh",
    "Library/Keychains",
    "Library/Cookies",
    "Library/Mail",
    "Library/Messages",
];

/// Routes to the person's files that are not their home folder: other
/// disks, the data volume under its own name, and shared temporary
/// folders (`/tmp` and `/var` are links into `/private`). Unreadable
/// unless the plan gives a folder in them back.
const SYSTEM_HIDDEN: [&str; 6] = [
    "/Volumes",
    // Any file by its volume and inode number.
    "/.vol",
    "/System/Volumes/Data",
    "/private/tmp",
    "/private/var/tmp",
    "/private/var/folders",
];

/// A path rule Seatbelt matches by pattern, without case: macOS volumes
/// usually ignore it, so `.GIT/hooks` is `.git/hooks` there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Pattern {
    /// The path and everything in it.
    Under(PathBuf),
    /// Anything named `name` at any depth in `root`, and everything in it.
    Named { root: PathBuf, name: String },
}

/// The sandbox's network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Net {
    Blocked,
    /// Only the helper's bridge to the netproxy socket at this path.
    Bridge(PathBuf),
    Open,
}

/// The sandbox of one session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Plan {
    /// Folders readable even where [`Plan::hidden`] hides them.
    pub(crate) read: Vec<PathBuf>,
    /// Files readable even where hidden.
    pub(crate) read_files: Vec<PathBuf>,
    /// Folders writable (and readable), except [`Plan::protected`].
    pub(crate) write: Vec<PathBuf>,
    /// Files writable (and readable).
    pub(crate) write_files: Vec<PathBuf>,
    /// Folders unreadable unless given back above.
    pub(crate) hidden: Vec<PathBuf>,
    /// Never read nor written, whatever gives them back.
    pub(crate) never: Vec<Pattern>,
    /// Never written.
    pub(crate) protected: Vec<Pattern>,
    /// The folder Unix sockets may be made and reached in: the session's
    /// temporary folder.
    pub(crate) sockets: Option<PathBuf>,
    pub(crate) net: Net,
    /// Variables the plan sets or removes.
    pub(crate) set: Vec<(OsString, Option<OsString>)>,
    /// Variables kept from Ovra's environment.
    pub(crate) kept: Vec<&'static str>,
    /// Whether the line's own variables win over the plan's (a language
    /// server points its caches into its cache folder).
    line_env_last: bool,
    pub(crate) program: PathBuf,
    pub(crate) cwd: PathBuf,
}

impl Plan {
    fn empty(program: PathBuf, cwd: PathBuf) -> Self {
        Self {
            read: Vec::new(),
            read_files: Vec::new(),
            write: Vec::new(),
            write_files: Vec::new(),
            hidden: Vec::new(),
            never: Vec::new(),
            protected: Vec::new(),
            sockets: None,
            net: Net::Blocked,
            set: Vec::new(),
            kept: Vec::new(),
            line_env_last: false,
            program,
            cwd,
        }
    }

    /// The sandbox of an agent's session.
    pub(crate) fn agent(
        setup: &Setup,
        profile: &SandboxProfile,
        line: &CommandLine,
    ) -> Result<Self> {
        let home = real(&setup.home)?;
        let data = real(&setup.data_dir).unwrap_or_else(|_| setup.data_dir.clone());
        let work_dir = real(&profile.work_dir)?;
        if !work_dir.is_dir() {
            return Err(invalid(format!("{} is not a folder", work_dir.display())));
        }
        match profile.kind {
            ProfileKind::Agent => {}
            // A pod's servers would listen on the Mac's own loopback: macOS
            // has no network namespace to give it one of its own.
            ProfileKind::Pod => {
                return Err(invalid("the macOS sandbox cannot run server pods yet"));
            }
            ProfileKind::Browser => {
                return Err(invalid(
                    "the macOS sandbox cannot run the agent browser yet",
                ));
            }
        }
        if !profile.expose.is_empty() {
            return Err(invalid("only a server pod exposes ports"));
        }
        for path in std::iter::once(&work_dir)
            .chain(&profile.read)
            .chain(&profile.write)
        {
            let path = real(path).unwrap_or_else(|_| path.clone());
            if home.starts_with(&path) || data.starts_with(&path) {
                return Err(invalid(format!(
                    "the sandbox does not let an agent into {}: it holds the home folder or Ovra's data",
                    path.display()
                )));
            }
        }
        for path in std::iter::once(&work_dir).chain(&profile.write) {
            let path = real(path).unwrap_or_else(|_| path.clone());
            if holds_agent_state(&path, &home) {
                return Err(invalid(format!(
                    "the sandbox does not let an agent write {}: it holds an agent's own settings",
                    path.display()
                )));
            }
        }
        let program = real(&line.program)?;
        let mut plan = Self::empty(program.clone(), real(&line.cwd)?);
        plan.hide(setup, &home, &data);
        if profile.work_dir_read_only {
            plan.read.push(work_dir.clone());
        } else {
            plan.write.push(work_dir.clone());
            plan.protect_work_dir(setup, &work_dir, &home);
        }
        for read in &profile.read {
            if let Ok(read) = real(read) {
                plan.read.push(read);
            }
        }
        for write in &profile.write {
            plan.write.push(real(write)?);
        }
        // An agent's sandbox: its own state, and its variables.
        let lists = profile.agent_state.as_ref().map(|state| state.lists);
        if let Some(state) = &profile.agent_state {
            let own = match &state.config_dir {
                Some(dir) => Some(account_dir(state.lists, dir, &home, &work_dir, setup)?),
                None => None,
            };
            plan.agent_state(state.lists, &home, &work_dir, own.as_deref());
            if let Some(own) = own {
                plan.set
                    .push((state.lists.config_var.into(), Some(own.into())));
            }
        }
        plan.kept = env_kept(lists);
        // No git credential helper: the keychain would hand out the
        // person's GitHub login (the profile also refuses to run
        // `git-credential-osxkeychain`).
        for (name, value) in [
            ("GIT_CONFIG_COUNT", "1"),
            ("GIT_CONFIG_KEY_0", "credential.helper"),
            ("GIT_CONFIG_VALUE_0", ""),
        ] {
            plan.set.push((name.into(), Some(value.into())));
        }
        plan.home_tools(setup, &home, &program);
        let caches = cache_key(profile.project.as_ref(), &work_dir);
        let temp = plan.private(setup, &caches, &cache_key(None, &work_dir))?;
        plan.sockets = Some(temp);
        match &profile.network {
            ProfileNetwork::Blocked => {}
            ProfileNetwork::Open => plan.net = Net::Open,
            ProfileNetwork::Proxy { address, ca_bundle } => {
                plan.net = Net::Bridge(real(Path::new(address))?);
                if let Some(bundle) = ca_bundle {
                    plan.trust(real(bundle)?);
                }
            }
        }
        if let Some(work) = &profile.work_env {
            plan.set
                .push(("OVRA_PORT".into(), Some(work.port.to_string().into())));
            plan.set.push((
                "OVRA_WORKTREE_ROOT".into(),
                Some(work.worktree_root.clone().into()),
            ));
            plan.set
                .push(("OVRA_MAIN_ROOT".into(), Some(work.main_root.clone().into())));
        }
        plan.finish();
        Ok(plan)
    }

    /// The sandbox of a language server (ADR 0007): the project read-only,
    /// its cache folder the only writable one, no agent's state, never a
    /// network or a Unix socket.
    pub(crate) fn server(
        setup: &Setup,
        profile: &ServerProfile,
        line: &CommandLine,
    ) -> Result<Self> {
        let home = real(&setup.home)?;
        let data = real(&setup.data_dir).unwrap_or_else(|_| setup.data_dir.clone());
        let project = real(&profile.project)?;
        let cache = real(&profile.cache_dir)?;
        if !cache.starts_with(data.join("lsp")) || cache == data.join("lsp") {
            return Err(invalid(format!(
                "a language server's cache folder must be in {}",
                data.join("lsp").display()
            )));
        }
        for path in std::iter::once(&project).chain(&profile.read) {
            let path = real(path).unwrap_or_else(|_| path.clone());
            if home.starts_with(&path) || data.starts_with(&path) {
                return Err(invalid(format!(
                    "the sandbox does not let a language server into {}: it holds the home folder or Ovra's data",
                    path.display()
                )));
            }
            if holds_agent_state(&path, &home) {
                return Err(invalid(format!(
                    "the sandbox does not let a language server read {}: it holds an agent's own settings",
                    path.display()
                )));
            }
        }
        let program = real(&line.program)?;
        let mut plan = Self::empty(program.clone(), project.clone());
        plan.hide(setup, &home, &data);
        plan.read.push(project);
        for read in &profile.read {
            if let Ok(read) = real(read) {
                plan.read.push(read);
            }
        }
        plan.write.push(cache.clone());
        for name in USER_CACHES {
            plan.read_tool(&home, &home.join(name));
        }
        plan.home_tools(setup, &home, &program);
        let temp = cache.join("tmp");
        fs::create_dir_all(&temp)
            .map_err(|e| invalid(format!("cannot make {}: {e}", temp.display())))?;
        plan.set.push(("TMPDIR".into(), Some(real(&temp)?.into())));
        plan.kept = env_kept(None);
        plan.line_env_last = true;
        if !matches!(profile.network, ProfileNetwork::Blocked) {
            return Err(invalid("a language server never gets a network"));
        }
        plan.finish();
        Ok(plan)
    }

    /// The home folder, Ovra's data and the other routes to the person's
    /// files, and the secrets in the home folder.
    fn hide(&mut self, setup: &Setup, home: &Path, data: &Path) {
        self.hidden.push(home.to_path_buf());
        self.hidden.push(data.to_path_buf());
        for path in setup
            .hidden
            .iter()
            .map(PathBuf::as_path)
            .chain(SYSTEM_HIDDEN.iter().map(Path::new))
        {
            self.hidden
                .push(real(path).unwrap_or_else(|_| path.to_path_buf()));
        }
        for name in NEVER {
            self.never.push(Pattern::Under(home.join(name)));
        }
    }

    /// The protected paths of the session's folder: those `ovra-sandbox`
    /// lists, every `.git` at any depth (a direct session's whole git
    /// folder, a nested repository's, one the agent would make), and what
    /// git reads from the folder on its own: the `core.hooksPath` folder,
    /// config files it includes, submodule folders that are not checked
    /// out. A protected name right in the folder that is a link keeps its
    /// target read-only too, since Seatbelt checks the path a write
    /// reaches.
    fn protect_work_dir(&mut self, setup: &Setup, work_dir: &Path, home: &Path) {
        let mut top = vec![".git".to_owned()];
        self.protected.push(Pattern::Named {
            root: work_dir.to_path_buf(),
            name: ".git".into(),
        });
        for pattern in setup.protected {
            match pattern.strip_prefix("**/") {
                Some(name) => {
                    self.protected.push(Pattern::Named {
                        root: work_dir.to_path_buf(),
                        name: name.into(),
                    });
                    top.push(name.into());
                }
                None => {
                    self.protected.push(Pattern::Under(work_dir.join(pattern)));
                    top.push((*pattern).into());
                }
            }
        }
        let mut paths: Vec<PathBuf> = hooks_path(work_dir, home).into_iter().collect();
        paths.extend(config_files(work_dir, home));
        paths.extend(empty_submodules(work_dir));
        paths.retain(|path| lexically_inside(path, work_dir));
        let links: Vec<PathBuf> = top
            .iter()
            .map(|name| work_dir.join(name))
            .chain(paths.iter().cloned())
            .filter(|path| fs::symlink_metadata(path).is_ok_and(|m| m.is_symlink()))
            .filter_map(|path| real(&path).ok())
            .collect();
        self.protected
            .extend(paths.into_iter().chain(links).map(Pattern::Under));
    }

    /// What the agent (`lists`) reads and writes of its own: its
    /// configuration folder read-only (an account's own folder `own`
    /// instead of the default one), and in it what it writes during a
    /// session. Seatbelt cannot make a folder private to the session, so
    /// only `lists.in_place` is writable, as on Windows: the folders the
    /// person's own sessions run commands from later stay read-only.
    fn agent_state(
        &mut self,
        lists: &ProviderLists,
        home: &Path,
        work_dir: &Path,
        own: Option<&Path>,
    ) {
        let (config, state) = match own {
            Some(dir) => (dir.to_path_buf(), dir.join(lists.state_file)),
            None => (home.join(lists.home_dir), home.join(lists.state_file)),
        };
        if own.is_none() {
            let _ = fs::create_dir_all(&config);
        }
        let Ok(config) = real(&config) else { return };
        if !config.starts_with(home) || config == home {
            return;
        }
        self.read.push(config.clone());
        for dir in lists.in_place {
            let dir = config.join(dir);
            if fs::create_dir_all(&dir).is_ok()
                && let Ok(dir) = real(&dir)
                && dir.starts_with(&config)
            {
                self.write.push(dir);
            }
        }
        // The transcripts of this folder's sessions, which a resume reads;
        // their memory loads into every later session there, the person's
        // own too, so it stays read-only like a rules file.
        let transcripts = config.join(lists.projects).join(project_key(work_dir));
        if fs::create_dir_all(transcripts.join(lists.project_memory)).is_ok()
            && let Ok(transcripts) = real(&transcripts)
            && transcripts.starts_with(&config)
        {
            self.protected
                .push(Pattern::Under(transcripts.join(lists.project_memory)));
            self.write.push(transcripts);
        }
        for name in lists.files {
            let file = config.join(name);
            if lists.files_made.contains(name) {
                let _ = fs::OpenOptions::new().create(true).append(true).open(&file);
            }
            if fs::symlink_metadata(&file).is_ok_and(|m| m.is_file()) {
                self.write_files.push(file);
            }
        }
        if fs::symlink_metadata(&state).is_ok_and(|m| m.is_file()) {
            self.write_files.push(state);
        }
    }

    /// What git and the toolchains read from the home folder: git's
    /// settings, the toolchain homes, the folders of `PATH`, and the
    /// program's own folder when it is installed under the home folder.
    fn home_tools(&mut self, setup: &Setup, home: &Path, program: &Path) {
        for name in HOME_READ {
            if let Ok(file) = real(&home.join(name))
                && file.starts_with(home)
                && file.is_file()
            {
                self.read_files.push(file);
            }
        }
        for toolchain in &setup.toolchains {
            self.read_tool(home, toolchain);
        }
        let mut kept = Vec::new();
        for entry in std::env::split_paths(&setup.path) {
            if !entry.is_absolute() || home.starts_with(&entry) {
                continue;
            }
            if let Ok(real_entry) = real(&entry)
                && real_entry.starts_with(home)
            {
                let toolchain = real_entry.parent().filter(|parent| {
                    real_entry.ends_with("bin")
                        && parent
                            .strip_prefix(home)
                            .ok()
                            .and_then(|rest| rest.components().next())
                            .is_some_and(|top| TOOLCHAINS.iter().any(|t| top.as_os_str() == *t))
                });
                let shown = toolchain.map_or_else(|| real_entry.clone(), Path::to_path_buf);
                self.read_tool(home, &shown);
            }
            kept.push(entry);
        }
        let path = std::env::join_paths(kept).unwrap_or_default();
        self.set.push(("PATH".into(), Some(path)));
        // `claude`'s package or versions folder, not a `bin` of the home
        // folder that holds other tools.
        let own_folder = program.parent().filter(|parent| {
            parent
                .strip_prefix(home)
                .is_ok_and(|rest| rest.components().count() >= 3)
                && !parent.ends_with("bin")
                && !parent.starts_with(home.join(".config"))
        });
        match own_folder {
            Some(folder) => self.read_tool(home, folder),
            None if program.starts_with(home) => self.read_files.push(program.to_path_buf()),
            None => {}
        }
    }

    /// Makes a folder of the home folder's tools readable, without the
    /// secrets toolchains keep in them (`~/.cargo/credentials.toml`).
    /// Outside the home folder it is readable anyway.
    fn read_tool(&mut self, home: &Path, path: &Path) {
        let Ok(path) = real(path) else { return };
        if !path.starts_with(home) || path == home {
            return;
        }
        let never = NEVER.iter().any(|n| path.starts_with(home.join(n)));
        if never || holds_agent_state(&path, home) {
            return;
        }
        for name in SECRETS {
            self.never.push(Pattern::Named {
                root: path.clone(),
                name: name.into(),
            });
        }
        self.read.push(path);
    }

    /// The project's own package caches (`caches`) and the work dir's own
    /// temporary folder (`temp`), in Ovra's data folder; returns the
    /// temporary folder. The temporary folder is not shared with other
    /// work dirs of the project: Unix sockets there are reachable.
    fn private(&mut self, setup: &Setup, caches: &str, temp: &str) -> Result<PathBuf> {
        let made = |dir: PathBuf| -> Result<PathBuf> {
            fs::create_dir_all(&dir)
                .map_err(|e| invalid(format!("cannot make {}: {e}", dir.display())))?;
            real(&dir)
        };
        for (name, folder) in CACHES {
            let dir = made(setup.caches.join(caches).join(folder))?;
            self.set.push((name.into(), Some(dir.clone().into())));
            self.write.push(dir);
        }
        let temp = made(setup.temp.join(temp))?;
        self.set.push(("TMPDIR".into(), Some(temp.clone().into())));
        self.write.push(temp.clone());
        Ok(temp)
    }

    /// Every TLS client trusts `bundle`, the netproxy's CA among the
    /// system's, and no folder of roots that would be read instead.
    fn trust(&mut self, bundle: PathBuf) {
        for name in CA_BUNDLE_VARS {
            self.set.push((name.into(), Some(bundle.clone().into())));
        }
        self.set.push(("SSL_CERT_DIR".into(), None));
        self.read_files.push(bundle);
    }

    fn finish(&mut self) {
        for list in [
            &mut self.read,
            &mut self.read_files,
            &mut self.write,
            &mut self.write_files,
            &mut self.hidden,
        ] {
            list.sort();
            list.dedup();
        }
    }

    /// The helper's command line: the profile and the variables to keep as
    /// arguments, then the line. `outer_env` holds the names in Ovra's
    /// environment now; every one the session does not keep is removed.
    pub(crate) fn line(
        &self,
        setup: &Setup,
        line: CommandLine,
        outer_env: &[OsString],
    ) -> Result<CommandLine> {
        let profile = crate::sbpl::profile(self)?;
        let mut args: Vec<OsString> = vec![HELPER_FLAG.into()];
        if let Net::Bridge(socket) = &self.net {
            args.extend(["--bridge".into(), socket.into()]);
        }
        let (mut env, last) = if self.line_env_last {
            (Vec::new(), line.env)
        } else {
            (line.env, Vec::new())
        };
        env.extend(self.set.iter().cloned());
        env.extend(last);
        for name in outer_env {
            let kept = self.kept.iter().any(|k| name == k);
            if !kept && !env.iter().any(|(n, _)| n == name) {
                env.push((name.clone(), None));
            }
        }
        // The helper passes on only these, whatever its own environment
        // gains later.
        let mut kept: Vec<OsString> = self.kept.iter().map(OsString::from).collect();
        for (name, value) in &env {
            if value.is_some() && !kept.contains(name) {
                kept.push(name.clone());
            }
        }
        for name in kept {
            let last = env.iter().rev().find(|(n, _)| *n == name);
            if last.is_none_or(|(_, value)| value.is_some()) {
                args.extend(["--env".into(), name]);
            }
        }
        args.extend(["--profile".into(), profile.into(), "--".into()]);
        args.push(self.program.clone().into());
        args.extend(line.args);
        Ok(CommandLine {
            program: setup.helper.clone(),
            args,
            env,
            cwd: self.cwd.clone(),
        })
    }
}

/// The variables a session of the agent `lists` describes keeps: its own
/// among everyone's; without an agent only everyone's.
fn env_kept(lists: Option<&'static ProviderLists>) -> Vec<&'static str> {
    ENV_PERSON
        .iter()
        .chain(lists.into_iter().flat_map(|l| l.env))
        .chain(&ENV_TOOLS)
        .chain(lists.map(|l| &l.fixture_var))
        .copied()
        .collect()
}

/// Whether `path` (resolved) is in or is one of the home folder's entries
/// whose name starts with one of [`AGENT_STATE_IN_HOME`], ignoring case: a
/// known agent's state or an account's folder such as `.claude-work`.
fn holds_agent_state(path: &Path, home: &Path) -> bool {
    let lower = |p: &Path| PathBuf::from(p.to_string_lossy().to_lowercase());
    lower(path)
        .strip_prefix(lower(home))
        .ok()
        .and_then(|rest| rest.components().next().map(|c| c.as_os_str().to_owned()))
        .is_some_and(|top| {
            let top = top.to_string_lossy();
            AGENT_STATE_IN_HOME.iter().any(|name| top.starts_with(name))
        })
}

/// The account's configuration folder, resolved, if the sandbox may give
/// it back: an existing folder right in the home folder whose name starts
/// with the agent's own folder's (`.claude`, not `.claude` itself), that
/// holds none of Ovra's data and does not overlap the work dir.
fn account_dir(
    lists: &ProviderLists,
    dir: &Path,
    home: &Path,
    work_dir: &Path,
    setup: &Setup,
) -> Result<PathBuf> {
    let refused = |why: &str| {
        Err(invalid(format!(
            "the sandbox does not use {} as a {} account's folder: {why}",
            dir.display(),
            lists.name,
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
        .is_some_and(|n| n.to_string_lossy().starts_with(lists.home_dir) && n != lists.home_dir);
    if real_dir.parent() != Some(home) || !named {
        return refused(&format!(
            "it must be a folder right in the home folder whose name starts with {0}, such as ~/{0}-work",
            lists.home_dir
        ));
    }
    if real_dir.starts_with(work_dir) || work_dir.starts_with(&real_dir) {
        return refused("it overlaps the session's folder");
    }
    let ovra = std::iter::once(&setup.data_dir)
        .chain(&setup.hidden)
        .filter_map(|h| real(h).ok())
        .any(|h| h.starts_with(&real_dir) || real_dir.starts_with(&h));
    if ovra {
        return refused("it holds Ovra's own data");
    }
    Ok(real_dir)
}

/// The name of `work_dir`'s folder among the agent's projects: every
/// character that is not a letter or digit becomes `-`, as `claude` does.
fn project_key(work_dir: &Path) -> String {
    work_dir
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// The folder name of a project's caches and temporary folder: a hash of
/// its id, so every work dir of the project shares one and no two projects
/// do. A profile that names no project gets one of its work dir's own.
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

/// `path` with every link resolved; it must exist.
pub(crate) fn real(path: &Path) -> Result<PathBuf> {
    fs::canonicalize(path).map_err(|e| invalid(format!("{}: {e}", path.display())))
}

/// What `git <args>` prints in `work_dir`, as the person's git sees it;
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

/// Config files git reads for `work_dir`: those that set something, and
/// those an `include.path` or `includeIf.*.path` names (an empty one could
/// be filled). Only those inside `work_dir` matter; the rest is read-only.
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
}

/// The folder `core.hooksPath` names, if set.
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

/// Submodule folders (gitlinks in `work_dir`'s index) that are not checked
/// out: the agent must not turn one into a link to a repository it made,
/// which git would step into. Only the work dir's own index is asked; a
/// checked-out submodule's `.git` is protected anyway.
fn empty_submodules(work_dir: &Path) -> Vec<PathBuf> {
    let text = git(work_dir, &["ls-files", "--stage", "-z"]).unwrap_or_default();
    text.split('\0')
        .filter_map(|record| {
            record
                .strip_prefix("160000 ")
                .and_then(|r| r.split_once('\t'))
        })
        .map(|(_, path)| work_dir.join(path))
        .filter(|dir| fs::symlink_metadata(dir.join(".git")).is_err())
        .collect()
}

/// Whether `path`, with `..` resolved by name, is inside `dir`.
fn lexically_inside(path: &Path, dir: &Path) -> bool {
    let mut normal = PathBuf::new();
    for part in path.components() {
        match part {
            Component::ParentDir => {
                normal.pop();
            }
            Component::CurDir => {}
            other => normal.push(other),
        }
    }
    normal.starts_with(dir) && normal != dir
}

#[cfg(test)]
mod tests;
