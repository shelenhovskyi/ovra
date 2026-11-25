//! What one language server's sandbox shows (ADR 0007, `docs/specs/lsp.md`).
//! Built from the agent plan's parts, without the agent's own state: the
//! project read-only, its cache folder the only writable one besides the
//! private temporary folders, the user's package caches read-only, and no
//! network at all.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use ovra_core::services::{ProfileNetwork, Result, ServerProfile};

use super::{CACHES, Kind, Mount, NEVER_BOUND, Plan, SECRETS, real};
use crate::{Setup, invalid};

/// The variables a language server or a server pod keeps from Ovra's
/// environment: those of [`super::ENV_KEPT`] without where Claude usage is
/// billed, the cloud provider's names and Ovra's own test recording. A
/// server has no use for them, and the project's code it runs must not see
/// them. Whole names
/// only, as for agents. `PATH` and `TMPDIR` are set by the plan, and the
/// server's own line sets its build and cache folders.
pub(super) const SERVER_ENV_KEPT: [&str; 19] = [
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

/// Paths under the home folder a server's `read` folder may neither be in
/// nor hold, besides the agent plan's [`NEVER_BOUND`] folders: Claude's
/// state and the files that keep credentials for tools.
const NEVER_READ: [&str; 7] = [
    ".claude",
    ".claude.json",
    ".netrc",
    ".git-credentials",
    ".npmrc",
    ".pypirc",
    ".pgpass",
];

impl Plan {
    /// The sandbox of one language server. Refuses a profile with any
    /// network, a project or folder that holds the home folder, a `read`
    /// folder of Ovra's or of secrets, and a cache folder that is in the
    /// project or outside the `lsp` folder of Ovra's data folder.
    pub(crate) fn server(setup: &Setup, profile: &ServerProfile) -> Result<Self> {
        if profile.network != ProfileNetwork::Blocked {
            return Err(invalid(
                "a language server runs without the network: the sandbox refuses any other profile",
            ));
        }
        let home = real(&setup.home)?;
        let project = folder(&profile.project)?;
        let cache = folder(&profile.cache_dir)?;
        let mut hidden: Vec<PathBuf> = setup.hidden.iter().filter_map(|h| real(h).ok()).collect();
        hidden.sort_by_key(|h| h.components().count());
        let holds_home = |path: &Path| {
            home.starts_with(path).then(|| {
                invalid(format!(
                    "the sandbox does not let a language server into {}: it holds the home folder",
                    path.display()
                ))
            })
        };
        let secret: Vec<PathBuf> = NEVER_BOUND
            .iter()
            .chain(&NEVER_READ)
            .map(|name| home.join(name))
            .collect();
        let mut read = Vec::new();
        for path in &profile.read {
            // As for agents, a folder that is not there is left out.
            let Ok(path) = real(path) else { continue };
            if let Some(error) = holds_home(&path) {
                return Err(error);
            }
            if hidden.iter().any(|h| path.starts_with(h)) {
                return Err(invalid(format!(
                    "the sandbox does not let a language server read {}: it is Ovra's",
                    path.display()
                )));
            }
            let named_secret = path
                .file_name()
                .is_some_and(|name| SECRETS.iter().any(|s| name == *s));
            if named_secret
                || secret
                    .iter()
                    .any(|s| path.starts_with(s) || s.starts_with(&path))
            {
                return Err(invalid(format!(
                    "the sandbox does not let a language server read {}: it holds secrets",
                    path.display()
                )));
            }
            read.push(path);
        }
        if let Some(error) = holds_home(&project).or_else(|| holds_home(&cache)) {
            return Err(error);
        }
        if cache.starts_with(&project) || project.starts_with(&cache) {
            return Err(invalid(format!(
                "a language server's cache folder {} must be outside the project, which it never writes",
                cache.display()
            )));
        }
        // Not just any hidden folder: the temporary folder holds the
        // netproxy's sockets, and the rest of the data folder is Ovra's.
        let caches = real(&setup.data_dir.join("lsp")).ok();
        if !caches.is_some_and(|c| cache.starts_with(&c) && cache != c) {
            return Err(invalid(format!(
                "a language server's cache folder {} must be inside the lsp folder of Ovra's data folder",
                cache.display()
            )));
        }
        let mut plan = Self {
            home: home.clone(),
            mounts: Vec::new(),
            // Not the home folder: it is empty, and nothing a server needs
            // is written there.
            writable: vec!["/tmp".into(), "/dev".into()],
            network: ProfileNetwork::Blocked,
            forward: Vec::new(),
            path: OsString::new(),
            temporary: Vec::new(),
            kind: Kind::Server {
                project: project.clone(),
            },
        };
        plan.hide(setup, &home);
        let shown: Vec<PathBuf> = [project.clone(), cache.clone()]
            .into_iter()
            .chain(read.iter().cloned())
            .collect();
        plan.home_read(setup, &home, &shown);
        plan.user_caches(&home);
        plan.cover_secrets(&home);
        plan.mounts.extend(read.into_iter().map(Mount::Ro));
        plan.mounts.push(Mount::Ro(project.clone()));
        plan.rw(cache.clone());
        // Ovra's hidden folders inside a folder bound above (a project that
        // holds `.ovra-data`) are hidden again, and the cache bound back.
        let before = plan.mounts.len();
        for hidden in hidden {
            let covered = plan.mounts[before..]
                .iter()
                .any(|m| matches!(m, Mount::Tmpfs(t) if hidden.starts_with(t)));
            let shown = plan.mounts[..before]
                .iter()
                .any(|m| matches!(m, Mount::Ro(p) if hidden.starts_with(p) && hidden != *p));
            if shown && !covered && !project.starts_with(&hidden) {
                plan.mounts.push(Mount::Tmpfs(hidden));
            }
        }
        let cache_hidden = plan.mounts[before..]
            .iter()
            .any(|m| matches!(m, Mount::Tmpfs(t) if cache.starts_with(t)));
        if cache_hidden {
            plan.mounts.push(Mount::Rw(cache));
        }
        Ok(plan)
    }

    /// The user's own package caches ([`CACHES`]), read-only where they
    /// are: a server reads the packages the person fetched, and never
    /// changes what their builds use. Nothing is made if missing.
    fn user_caches(&mut self, home: &Path) {
        for name in CACHES {
            if let Ok(cache) = real(&home.join(name))
                && cache.is_dir()
                && cache.starts_with(home)
                && !home.starts_with(&cache)
            {
                self.ro(cache);
            }
        }
    }
}

/// `path` with links resolved; it must be a folder.
fn folder(path: &Path) -> Result<PathBuf> {
    let path = real(path)?;
    if !path.is_dir() {
        return Err(invalid(format!("{} is not a folder", path.display())));
    }
    Ok(path)
}
