//! What one session's sandbox may touch on Windows: the rights the sandbox
//! user and the project's SID get (ACEs), the protected paths they are
//! denied, and the environment `claude` starts with. Planned from paths
//! alone, so it is tested on every OS; `acl` applies it on Windows.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};

use ovra_core::protected::AGENT_STATE_IN_HOME;
use ovra_core::services::{CommandLine, ProfileNetwork, ProviderLists, Result, SandboxProfile};

use crate::{HELPER_FLAG, Setup, invalid};

/// The variables a session keeps from Ovra's environment besides its
/// agent's own ([`ProviderLists::env`] and `windows_env`), as on Linux
/// (`ENV_PERSON` and `ENV_TOOLS` in `sandbox-linux`), plus what Windows
/// programs need to start. Whole names only, compared without case as
/// Windows does: a prefix such as `CLAUDE_CODE_` would let
/// `CLAUDE_CODE_OAUTH_TOKEN` through. `PATH`, `TEMP`, `TMP`, the proxy and
/// cache variables are set by the plan itself. First the person's own and
/// their terminal's, then ([`ENV_TOOLS`]) what their tools and Windows
/// need.
const ENV_PERSON: [&str; 9] = [
    "USERNAME",
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
const ENV_TOOLS: [&str; 25] = [
    // Certificate bundles, for networks that inspect TLS.
    "SSL_CERT_FILE",
    "NODE_EXTRA_CA_CERTS",
    // Toolchain homes.
    "RUSTUP_HOME",
    "NVM_HOME",
    "NVM_SYMLINK",
    "GOPATH",
    // Windows itself. `USERPROFILE` stays the person's, so the agent finds
    // its login there.
    "SystemRoot",
    "SystemDrive",
    "windir",
    "ComSpec",
    "PATHEXT",
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
    "APPDATA",
    "LOCALAPPDATA",
    "ProgramData",
    "ProgramFiles",
    "ProgramFiles(x86)",
    "ProgramW6432",
    "CommonProgramFiles",
    "CommonProgramFiles(x86)",
    "NUMBER_OF_PROCESSORS",
    "PROCESSOR_ARCHITECTURE",
    "OS",
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
/// pointed at the copy; the person's own caches stay unreadable.
/// `CARGO_HOME` holds only the registry and git checkouts then: `cargo`
/// itself is found on `PATH` (`~/.cargo/bin`).
const CACHES: [(&str, &str); 4] = [
    ("CARGO_HOME", "cargo"),
    ("npm_config_cache", "npm"),
    ("PIP_CACHE_DIR", "pip"),
    ("npm_config_store_dir", "pnpm"),
];

/// Files that stay readable from the home folder. Not all of
/// `~/.config/git`: git's credential store keeps `credentials` there.
const HOME_READ: [&str; 4] = [
    ".gitconfig",
    ".config/git/config",
    ".config/git/ignore",
    ".config/git/attributes",
];

/// Folders under the home folder whose `bin` on `PATH` brings the whole
/// folder (the toolchain's libraries live next to it). Any other entry is
/// granted alone.
const TOOLCHAINS: [&str; 9] = [
    ".cargo", ".rustup", ".bun", ".deno", "go", ".pyenv", "scoop", ".volta", ".yarn",
];

/// Folders under the home folder no grant ever reaches, whatever the depth.
const NEVER_GRANTED: [&str; 7] = [
    ".config/gh",
    ".ssh",
    ".aws",
    ".gnupg",
    ".docker",
    ".kube",
    "AppData/Roaming/Microsoft",
];

/// Protected folders made empty in the session's folder when missing, so
/// the agent cannot create them for the user's own tools to load later.
const FOLDERS_MADE: [&str; 4] = [".claude", ".vscode", ".idea", ".devcontainer"];

/// What a grant lets the sandbox do with a folder or file (and, for a
/// folder, everything in it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Right {
    /// Read, write, create and delete; never delete-child, so a protected
    /// entry cannot be removed through its parent.
    Modify,
    Read,
    ReadExecute,
}

impl Right {
    pub(crate) fn flag(self) -> &'static str {
        match self {
            Self::Modify => "m",
            Self::Read => "r",
            Self::ReadExecute => "rx",
        }
    }
}

/// The sandbox of one session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Plan {
    /// Sorted, one right per path.
    pub(crate) grants: Vec<(PathBuf, Right)>,
    /// Protected paths: write, delete, ACL and owner changes are denied.
    pub(crate) denied: Vec<PathBuf>,
    /// The session's `PATH`.
    pub(crate) path: OsString,
    /// Variables the plan sets (the cache, temporary and proxy ones).
    pub(crate) set: Vec<(OsString, Option<OsString>)>,
    /// The project's SID ([`project_sid`]).
    pub(crate) sid: String,
    /// The variables the session keeps from Ovra's environment.
    pub(crate) kept: Vec<&'static str>,
}

impl Plan {
    pub(crate) fn new(setup: &Setup, profile: &SandboxProfile, program: &Path) -> Result<Self> {
        let home = real(&setup.home)?;
        let work_dir = real(&profile.work_dir)?;
        if !work_dir.is_dir() {
            return Err(invalid(format!("{} is not a folder", work_dir.display())));
        }
        // Server pods (ADR 0008) have no Windows bridge yet. A pod is
        // refused; an agent's `forward` is left out, which only takes away
        // reach (it cannot get to its work dir's servers).
        if !profile.expose.is_empty() {
            return Err(invalid("the Windows sandbox cannot run server pods yet"));
        }
        // Grants stay on a folder after its sessions (the project's SID is
        // the same for each), so an earlier direct session's write access
        // would still apply: a read-only folder needs a SID of its own first.
        if profile.work_dir_read_only {
            return Err(invalid(
                "the Windows sandbox cannot keep a folder read-only yet, so research agents cannot run in it",
            ));
        }
        // It would show the default folder instead of the account's.
        let lists = profile.agent_state.as_ref().map(|state| state.lists);
        if let Some(state) = &profile.agent_state
            && state.config_dir.is_some()
        {
            return Err(invalid(format!(
                "the Windows sandbox cannot run a {} account with its own folder yet; use the default account",
                state.lists.name
            )));
        }
        let mut plan = Self {
            grants: Vec::new(),
            denied: Vec::new(),
            path: OsString::new(),
            set: Vec::new(),
            sid: project_sid(&work_dir),
            kept: env_kept(lists),
        };
        let data = real(&setup.data_dir).unwrap_or_else(|_| setup.data_dir.clone());
        for path in std::iter::once(&profile.work_dir)
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
        for path in std::iter::once(&profile.work_dir).chain(&profile.write) {
            let path = real(path).unwrap_or_else(|_| path.clone());
            if AGENT_STATE_IN_HOME
                .iter()
                .any(|name| within(&path, &home.join(name)))
            {
                return Err(invalid(format!(
                    "the sandbox does not let an agent write {}: it holds an agent's own settings",
                    path.display()
                )));
            }
        }
        plan.grant(work_dir.clone(), Right::Modify);
        for write in &profile.write {
            plan.grant(real(write)?, Right::Modify);
        }
        for read in &profile.read {
            if let Ok(read) = real(read) {
                plan.grant(read, Right::Read);
            }
        }
        if let Some(lists) = lists {
            plan.agent_state(lists, &home, &work_dir);
        }
        plan.program(&home, program);
        // The sandbox user starts the helper too.
        plan.program(&home, &setup.helper);
        plan.path = plan.path_entries(&home, &setup.path);
        plan.private(setup, &work_dir)?;
        if let ProfileNetwork::Proxy {
            address,
            ca_bundle,
            variables,
        } = &profile.network
        {
            let proxy = OsString::from(format!("http://{address}"));
            // Names are one to Windows whatever their case.
            for name in ["HTTP_PROXY", "HTTPS_PROXY"] {
                plan.set.push((name.into(), Some(proxy.clone())));
            }
            for name in ["NO_PROXY", "ALL_PROXY"] {
                plan.set.push((name.into(), None));
            }
            plan.set
                .push(("NODE_USE_ENV_PROXY".into(), Some("1".into())));
            if let Some(bundle) = ca_bundle {
                let bundle = real(bundle)?;
                plan.grant(bundle.clone(), Right::Read);
                for name in CA_BUNDLE_VARS {
                    plan.set.push((name.into(), Some(bundle.clone().into())));
                }
                plan.set.push(("SSL_CERT_DIR".into(), None));
            }
            // Keys' placeholders (`docs/specs/secrets-consent.md`).
            for (name, placeholder) in variables {
                plan.set.push((name.into(), Some(placeholder.into())));
            }
        }
        plan.denied
            .extend(crate::protected::paths(setup, &work_dir, &FOLDERS_MADE)?);
        plan.denied.sort();
        plan.grants.sort();
        plan.grants.dedup_by(|a, b| a.0 == b.0);
        Ok(plan)
    }

    fn grant(&mut self, path: PathBuf, right: Right) {
        self.grants.push((path, right));
    }

    /// What the agent (`lists`) reads and writes of its own in the home
    /// folder, and the git settings there. On Windows the folders it writes
    /// cannot be private to the session: the sandbox user writes the
    /// person's own, so only `lists.in_place` are writable.
    fn agent_state(&mut self, lists: &ProviderLists, home: &Path, work_dir: &Path) {
        let config = home.join(lists.home_dir);
        let _ = fs::create_dir_all(&config);
        let Ok(config) = real(&config).map(|c| c.starts_with(home).then_some(c)) else {
            return;
        };
        let Some(config) = config else { return };
        self.grant(config.clone(), Right::Read);
        for dir in lists.in_place {
            let dir = config.join(dir);
            if fs::create_dir_all(&dir).is_ok()
                && let Ok(dir) = real(&dir)
                && dir.starts_with(&config)
            {
                self.grant(dir, Right::Modify);
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
            self.grant(transcripts.clone(), Right::Modify);
            self.denied.push(transcripts.join(lists.project_memory));
        }
        for name in lists.files {
            let file = config.join(name);
            if lists.files_made.contains(name) {
                let _ = fs::OpenOptions::new().create(true).append(true).open(&file);
            }
            if fs::symlink_metadata(&file).is_ok_and(|m| m.is_file()) {
                self.grant(file, Right::Modify);
            }
        }
        let state = home.join(lists.state_file);
        if fs::symlink_metadata(&state).is_ok_and(|m| m.is_file()) {
            self.grant(state, Right::Modify);
        }
        for read in HOME_READ {
            if let Ok(read) = real(&home.join(read))
                && read.starts_with(home)
                && read.is_file()
            {
                self.grant(read, Right::Read);
            }
        }
    }

    /// `claude`'s own folder when it is installed under the home folder: the
    /// folder of `claude.exe`, or of `claude.cmd` with its `node_modules`.
    fn program(&mut self, home: &Path, program: &Path) {
        let Ok(program) = real(program) else { return };
        let Some(folder) = program.parent() else {
            return;
        };
        if folder.starts_with(home) && folder != home && !never_granted(home, folder) {
            self.grant(folder.to_path_buf(), Right::ReadExecute);
        }
    }

    /// The session's `PATH`: Ovra's, without relative entries and without
    /// the home folder or its ancestors. Entries under the home folder are
    /// granted read and execute: a toolchain's `bin` with its toolchain,
    /// any other alone.
    fn path_entries(&mut self, home: &Path, path: &OsStr) -> OsString {
        let mut kept = Vec::new();
        for entry in std::env::split_paths(path) {
            if !entry.is_absolute() || home.starts_with(&entry) {
                continue;
            }
            if let Ok(real_entry) = real(&entry)
                && real_entry.starts_with(home)
            {
                if never_granted(home, &real_entry) {
                    continue;
                }
                let toolchain = real_entry.parent().filter(|parent| {
                    real_entry.ends_with("bin")
                        && parent
                            .strip_prefix(home)
                            .ok()
                            .and_then(|rest| rest.components().next())
                            .is_some_and(|top| TOOLCHAINS.iter().any(|t| top.as_os_str() == *t))
                });
                let granted = toolchain.map_or_else(|| real_entry.clone(), Path::to_path_buf);
                if granted != home {
                    self.grant(granted, Right::ReadExecute);
                }
            }
            kept.push(entry);
        }
        std::env::join_paths(kept).unwrap_or_default()
    }

    /// The project's own package caches and temporary folder, in Ovra's
    /// data folder.
    fn private(&mut self, setup: &Setup, work_dir: &Path) -> Result<()> {
        let key = project_dir(work_dir);
        let made = |dir: PathBuf| -> Result<PathBuf> {
            fs::create_dir_all(&dir)
                .map_err(|e| invalid(format!("cannot make {}: {e}", dir.display())))?;
            real(&dir)
        };
        for (name, folder) in CACHES {
            let dir = made(setup.caches.join(&key).join(folder))?;
            self.grant(dir.clone(), Right::Modify);
            self.set.push((name.into(), Some(dir.into())));
        }
        let temp = made(setup.temp.join(&key))?;
        self.grant(temp.clone(), Right::Modify);
        for name in ["TEMP", "TMP"] {
            self.set.push((name.into(), Some(temp.clone().into())));
        }
        Ok(())
    }

    /// The helper's command line: the plan as arguments, then the line.
    /// `outer_env` holds the names in Ovra's environment now; every one the
    /// session does not keep is removed, so the helper's own environment
    /// holds none of them either.
    pub(crate) fn line(
        &self,
        setup: &Setup,
        line: CommandLine,
        outer_env: &[OsString],
    ) -> CommandLine {
        let mut args: Vec<OsString> = vec![
            HELPER_FLAG.into(),
            "--sid".into(),
            (&self.sid).into(),
            "--data".into(),
            (&setup.data_dir).into(),
        ];
        for (path, right) in &self.grants {
            args.extend([OsString::from(format!("--{}", right.flag())), path.into()]);
        }
        for path in &self.denied {
            args.extend([OsString::from("--deny"), path.into()]);
        }
        let mut env = line.env;
        env.extend(self.set.iter().cloned());
        env.push(("PATH".into(), Some(self.path.clone())));
        for name in outer_env {
            let kept = self.kept.iter().any(|k| same_name(name, k));
            if !kept && !env.iter().any(|(n, _)| same_name(n, name)) {
                env.push((name.clone(), None));
            }
        }
        // The helper passes on only these, whatever its own environment
        // gains later.
        let mut kept: Vec<OsString> = self.kept.iter().map(OsString::from).collect();
        for (name, value) in &env {
            if value.is_some() && !kept.iter().any(|k| same_name(k, name)) {
                kept.push(name.clone());
            }
        }
        for name in kept {
            let last = env.iter().rev().find(|(n, _)| same_name(n, &name));
            if last.is_none_or(|(_, value)| value.is_some()) {
                args.extend([OsString::from("--env"), name]);
            }
        }
        args.push("--".into());
        args.push(line.program.into());
        args.extend(line.args);
        CommandLine {
            program: setup.helper.clone(),
            args,
            env,
            cwd: line.cwd,
        }
    }
}

/// The variables a session of the agent `lists` describes keeps: its own
/// among everyone's; without an agent only everyone's.
fn env_kept(lists: Option<&'static ProviderLists>) -> Vec<&'static str> {
    ENV_PERSON
        .iter()
        .chain(
            lists
                .into_iter()
                .flat_map(|l| l.env.iter().chain(l.windows_env)),
        )
        .chain(&ENV_TOOLS)
        .chain(lists.map(|l| &l.fixture_var))
        .copied()
        .collect()
}

/// Whether `path` is `base` or inside it, ignoring case as Windows does:
/// `~/.SSH` is `~/.ssh` there.
fn within(path: &Path, base: &Path) -> bool {
    let fold = |p: &Path| PathBuf::from(p.to_string_lossy().to_lowercase());
    fold(path).starts_with(fold(base))
}

/// Whether two variable names are the same to Windows.
fn same_name(a: &OsStr, b: impl AsRef<OsStr>) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// Whether `path` lies in a folder of the home folder no grant reaches.
fn never_granted(home: &Path, path: &Path) -> bool {
    NEVER_GRANTED.iter().any(|n| within(path, &home.join(n)))
}

/// The name of `work_dir`'s folder among `claude`'s projects: every
/// character that is not a letter or digit becomes `-`, as `claude` does.
pub(crate) fn project_key(work_dir: &Path) -> String {
    work_dir
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// A hash of the project's folder, its identity for Ovra: the same folder
/// whatever the case of its name (Windows treats names so), and a
/// different one for any other folder. [`project_key`] would not do:
/// `a-b`, `a b` and `a/b` share a key.
fn project_hash(work_dir: &Path) -> [u8; 32] {
    use sha2::Digest as _;
    let text = work_dir.to_string_lossy().to_lowercase();
    sha2::Sha256::digest(text.as_bytes()).into()
}

/// The name of the project's own folders (caches, temp) in Ovra's data.
pub(crate) fn project_dir(work_dir: &Path) -> String {
    project_hash(work_dir)[..12]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The SID that stands for one project in ACEs and in the session's
/// token: `S-1-5-21-` and four numbers from [`project_hash`]. It names no
/// account; only Ovra's grants mention it.
pub(crate) fn project_sid(work_dir: &Path) -> String {
    let hash = project_hash(work_dir);
    let part = |i: usize| u32::from_le_bytes([hash[i], hash[i + 1], hash[i + 2], hash[i + 3]]);
    format!("S-1-5-21-{}-{}-{}-{}", part(0), part(4), part(8), part(12))
}

/// `path` with links resolved, absolute.
pub(crate) fn real(path: &Path) -> Result<PathBuf> {
    let real = dunce_canonical(path)
        .map_err(|e| invalid(format!("cannot use {}: {e}", path.display())))?;
    Ok(real)
}

/// `fs::canonicalize`, without the `\\?\` prefix Windows adds, so paths
/// compare with the ones the person and `claude` use.
fn dunce_canonical(path: &Path) -> std::io::Result<PathBuf> {
    let real = fs::canonicalize(path)?;
    let text = real.to_string_lossy();
    Ok(match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC\\") => PathBuf::from(rest),
        _ => real,
    })
}

#[cfg(test)]
mod tests;
