//! Language servers for trusted projects (ADR 0007, `docs/specs/lsp.md`):
//! finds the servers Ovra knows on `PATH`, starts them through the
//! sandbox, and frames their JSON-RPC messages. Trust, URIs and the
//! requests a server makes are the core's (`crates/core/src/lsp/`).
//! Contract: `ovra_core::services::LanguageServers`; fake:
//! `ovra_core::fakes::FakeLanguageServers`.

mod env;
mod frame;
mod known;
mod process;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ovra_core::protocol::lsp::KnownServer;
use ovra_core::protocol::projects::ProjectId;
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{
    BoxFuture, CommandLine, LanguageServers, ProfileNetwork, Result, Sandbox, SandboxStatus,
    ServerHost, ServerProfile, ServerSpec, StartedServer,
};

pub use crate::known::ServerDef;

/// The language servers of this machine.
#[derive(Debug)]
pub struct Servers {
    sandbox: Arc<dyn Sandbox>,
    /// `<data dir>/lsp`: one cache folder per project and language.
    caches: PathBuf,
    defs: Vec<ServerDef>,
}

impl Servers {
    /// The servers Ovra knows (`known.rs`).
    pub fn new(sandbox: Arc<dyn Sandbox>, caches: PathBuf) -> Self {
        Self::with_known(sandbox, caches, known::table())
    }

    /// Only the servers in `defs`, such as a fake one for tests.
    pub fn with_known(sandbox: Arc<dyn Sandbox>, caches: PathBuf, defs: Vec<ServerDef>) -> Self {
        Self {
            sandbox,
            caches,
            defs,
        }
    }
}

impl LanguageServers for Servers {
    fn known(&self) -> BoxFuture<'_, Vec<KnownServer>> {
        Box::pin(async move {
            let defs = self.defs.clone();
            let path = std::env::var_os("PATH");
            tokio::task::spawn_blocking(move || known::known(&defs, path.as_deref()))
                .await
                .unwrap_or_default()
        })
    }

    fn sandbox(&self) -> Option<String> {
        match self.sandbox.status() {
            SandboxStatus::Available { name } => Some(name),
            SandboxStatus::Unavailable { .. } => None,
        }
    }

    fn start(&self, spec: ServerSpec, host: ServerHost) -> BoxFuture<'_, Result<StartedServer>> {
        Box::pin(async move {
            let path = std::env::var_os("PATH");
            let (def, program) = known::installed(&self.defs, &spec.language, path.as_deref())
                .ok_or_else(|| {
                    ProtocolError::new(
                        ErrorCode::NotFound,
                        format!("no language server is installed for {}", spec.language),
                    )
                })?;
            let cache = cache_dir(&self.caches, &spec)?;
            let line = line(def, program, &spec.root, &cache);
            let (line, sandbox) = match self.sandbox.status() {
                SandboxStatus::Available { name } => {
                    let profile = ServerProfile {
                        project: spec.root.clone(),
                        cache_dir: cache,
                        read: Vec::new(),
                        network: ProfileNetwork::Blocked,
                    };
                    // Never falls back to running it unsandboxed.
                    let line = self.sandbox.wrap_server(&profile, line).map_err(|e| {
                        ProtocolError::new(
                            e.code,
                            format!("the sandbox cannot run {}: {}", def.name, e.message),
                        )
                    })?;
                    (line, Some(name))
                }
                // Without a sandbox, only the names it needs.
                SandboxStatus::Unavailable { .. } => {
                    let outer = std::env::vars_os().map(|(name, _)| name);
                    (env::unsandboxed(line, outer, cfg!(windows)), None)
                }
            };
            let process = process::spawn(&line, &def.name, host)?;
            Ok(StartedServer {
                process: Box::new(process),
                name: def.name.clone(),
                sandbox,
            })
        })
    }

    fn forget(&self, project: &ProjectId) {
        let _ = forget(&self.caches, project);
    }
}

/// `<caches>/<project id>/<language>`, created with the folders the
/// server's environment points at. Any of them that is not a real folder
/// (a link a server planted, a file) is replaced by one, so a later start
/// never writes elsewhere.
fn cache_dir(caches: &Path, spec: &ServerSpec) -> Result<PathBuf> {
    if !plain(&spec.project.0) || !plain(&spec.language) {
        return Err(ProtocolError::new(
            ErrorCode::BadRequest,
            "not a project and language a cache folder can be named after",
        ));
    }
    let failed = |e: std::io::Error| {
        ProtocolError::new(
            ErrorCode::Internal,
            format!("could not create {}: {e}", caches.display()),
        )
    };
    std::fs::create_dir_all(caches).map_err(failed)?;
    let project = caches.join(&spec.project.0);
    let cache = project.join(&spec.language);
    real_dir(&project).map_err(failed)?;
    real_dir(&cache).map_err(failed)?;
    for sub in ["target", "cache", "tmp"] {
        real_dir(&cache.join(sub)).map_err(failed)?;
    }
    Ok(cache)
}

/// A name a cache folder can have: one plain path segment.
fn plain(name: &str) -> bool {
    // Letters, digits and `-_.+` only: no separator, and no `C:` that
    // would make `join` leave the caches folder on Windows.
    let allowed = |c: char| c.is_ascii_alphanumeric() || "-_.+".contains(c);
    !name.is_empty() && name != "." && name != ".." && name.chars().all(allowed)
}

/// Makes `path` a folder of its own: kept if it is one, else whatever is
/// there (never a folder, so only a link or a file) is removed first.
fn real_dir(path: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => return Ok(()),
        Ok(_) => std::fs::remove_file(path)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    std::fs::create_dir(path)
}

/// Deletes `<caches>/<project id>`, Ovra's own folder and never the
/// project's. It is moved aside at once, so a server started after this
/// gets a new one, and deleted in the background when a runtime runs.
fn forget(caches: &Path, project: &ProjectId) -> std::io::Result<()> {
    if !plain(&project.0) {
        return Ok(());
    }
    let folder = caches.join(&project.0);
    let meta = match std::fs::symlink_metadata(&folder) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    if !meta.is_dir() {
        return std::fs::remove_file(&folder);
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let aside = caches.join(format!(".forgotten-{}-{nanos}", project.0));
    std::fs::rename(&folder, &aside)?;
    // `remove_dir_all` removes links, never what they point at.
    match tokio::runtime::Handle::try_current() {
        Ok(runtime) => {
            runtime.spawn_blocking(move || std::fs::remove_dir_all(aside));
            Ok(())
        }
        Err(_) => std::fs::remove_dir_all(aside),
    }
}

/// The server's command line before a sandbox wraps it. Build output and
/// caches go to `cache`, never into the project.
fn line(def: &ServerDef, program: PathBuf, root: &Path, cache: &Path) -> CommandLine {
    let set = |key: &str, value: OsString| (OsString::from(key), Some(value));
    let mut env = vec![
        set("CARGO_TARGET_DIR", cache.join("target").into()),
        set("XDG_CACHE_HOME", cache.join("cache").into()),
        set("TMPDIR", cache.join("tmp").into()),
        set("CARGO_NET_OFFLINE", "true".into()),
    ];
    env.extend(def.env.iter().map(|(k, v)| set(k, v.into())));
    CommandLine {
        program,
        args: def.args.iter().map(OsString::from).collect(),
        env,
        cwd: root.to_path_buf(),
    }
}

#[cfg(test)]
mod plain_tests {
    #[test]
    fn only_plain_names_become_cache_folders() {
        for bad in ["", ".", "..", "a/b", "a\\b", "C:", "c:x", "a b"] {
            assert!(!super::plain(bad), "{bad}");
        }
        for good in ["p1", "rust", "c++", "9f2c_a-b.c"] {
            assert!(super::plain(good), "{good}");
        }
    }
}

#[cfg(test)]
mod tests {
    use ovra_core::fakes::FakeSandbox;

    use super::*;

    fn spec(language: &str) -> ServerSpec {
        ServerSpec {
            project: ProjectId("p1".into()),
            root: ovra_core::test_path!("/work/a").into(),
            language: language.into(),
        }
    }

    /// A server whose program exists but is never run.
    fn installed(dir: &Path) -> ServerDef {
        let program = dir.join("ls");
        std::fs::write(&program, "").expect("write");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::Permissions::from_mode(0o755);
            std::fs::set_permissions(&program, mode).expect("chmod");
        }
        ServerDef {
            language: "rust".into(),
            name: "fake-ls".into(),
            program,
            args: vec!["--stdio".into()],
            install: "install it".into(),
            env: vec![("GOPROXY".into(), "off".into())],
        }
    }

    #[test]
    fn the_line_points_caches_and_build_output_at_the_cache() {
        let def = installed(tempfile::tempdir().expect("tmp").path());
        let cache = Path::new("/data/lsp/p1/rust");
        let line = line(&def, "/bin/ls".into(), Path::new("/work/a"), cache);
        let env: Vec<(String, String)> = line
            .env
            .iter()
            .map(|(k, v)| {
                let v = v.as_ref().map(|v| v.to_string_lossy().into_owned());
                (k.to_string_lossy().into_owned(), v.unwrap_or_default())
            })
            .collect();
        let pairs: Vec<(&str, &str)> = env.iter().map(|(k, v)| (&**k, &**v)).collect();
        assert_eq!(
            pairs,
            [
                ("CARGO_TARGET_DIR", &*cache.join("target").to_string_lossy()),
                ("XDG_CACHE_HOME", &*cache.join("cache").to_string_lossy()),
                ("TMPDIR", &*cache.join("tmp").to_string_lossy()),
                ("CARGO_NET_OFFLINE", "true"),
                ("GOPROXY", "off"),
            ]
        );
        assert_eq!(line.program, Path::new("/bin/ls"));
        assert_eq!(line.args, ["--stdio"]);
        assert_eq!(line.cwd, Path::new("/work/a"));
    }

    #[tokio::test]
    async fn a_sandbox_that_cannot_wrap_the_server_means_no_start() {
        let dir = tempfile::tempdir().expect("tmp");
        let sandbox = Arc::new(FakeSandbox {
            fails: true,
            ..FakeSandbox::available()
        });
        let caches = dir.path().join("lsp");
        let servers =
            Servers::with_known(sandbox.clone(), caches.clone(), vec![installed(dir.path())]);
        let (host, record) = ServerHost::recording();
        let error = servers
            .start(spec("rust"), host)
            .await
            .expect_err("refused");
        assert_eq!(error.code, ErrorCode::Unavailable);
        assert!(error.message.contains("fake-ls"), "{}", error.message);
        let cache_dir = caches.join("p1").join("rust");
        let profiles = sandbox.servers.lock().expect("lock").clone();
        assert_eq!(
            profiles,
            [ServerProfile {
                project: spec("rust").root,
                cache_dir: cache_dir.clone(),
                read: vec![],
                network: ProfileNetwork::Blocked,
            }]
        );
        assert!(cache_dir.join("tmp").is_dir());
        assert_eq!(record.exit(), None);
    }

    #[tokio::test]
    async fn names_the_sandbox_and_refuses_languages_without_a_server() {
        let dir = tempfile::tempdir().expect("tmp");
        let servers = Servers::with_known(
            Arc::new(FakeSandbox::available()),
            dir.path().into(),
            vec![installed(dir.path())],
        );
        assert_eq!(servers.sandbox().as_deref(), Some("fake"));
        let (host, _) = ServerHost::recording();
        let code = servers.start(spec("go"), host).await.err().map(|e| e.code);
        assert_eq!(code, Some(ErrorCode::NotFound));
        let mut bad = spec("rust");
        bad.project = ProjectId("..".into());
        let (host, _) = ServerHost::recording();
        let code = servers.start(bad, host).await.err().map(|e| e.code);
        assert_eq!(code, Some(ErrorCode::BadRequest));
        let known = servers.known().await;
        assert_eq!(known.len(), 1);
        assert!(known[0].path.is_some());
    }

    #[cfg(unix)]
    #[test]
    fn links_planted_in_the_cache_are_replaced_by_folders() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().expect("tmp");
        let caches = dir.path().join("lsp");
        let elsewhere = dir.path().join("elsewhere");
        std::fs::create_dir_all(&elsewhere).expect("mkdir");
        std::fs::create_dir_all(caches.join("p1/rust")).expect("mkdir");
        symlink(&elsewhere, caches.join("p1/rust/target")).expect("link");
        std::fs::write(caches.join("p1/rust/tmp"), "").expect("write");
        symlink(&elsewhere, caches.join("p2")).expect("link");
        let mut other = spec("rust");
        other.project = ProjectId("p2".into());
        for spec in [spec("rust"), other] {
            let cache = cache_dir(&caches, &spec).expect("cache");
            for path in [cache.clone(), cache.join("target"), cache.join("tmp")] {
                let meta = std::fs::symlink_metadata(&path).expect("meta");
                assert!(meta.is_dir(), "{path:?}");
            }
            assert!(cache.starts_with(&caches));
        }
        assert_eq!(std::fs::read_dir(&elsewhere).expect("list").count(), 0);
        // A link in place of the cache folder itself is replaced too.
        std::fs::remove_dir_all(caches.join("p1/rust")).expect("rm");
        symlink(&elsewhere, caches.join("p1/rust")).expect("link");
        cache_dir(&caches, &spec("rust")).expect("cache");
        assert!(
            !std::fs::symlink_metadata(caches.join("p1/rust"))
                .expect("meta")
                .is_symlink()
        );
        assert_eq!(std::fs::read_dir(&elsewhere).expect("list").count(), 0);
    }

    #[test]
    fn forgetting_a_project_deletes_only_its_cache_folder() {
        let dir = tempfile::tempdir().expect("tmp");
        let caches = dir.path().join("lsp");
        let outside = dir.path().join("project/src/main.rs");
        std::fs::create_dir_all(outside.parent().expect("parent")).expect("mkdir");
        std::fs::write(&outside, "fn main() {}").expect("write");
        let cache = cache_dir(&caches, &spec("rust")).expect("cache");
        std::fs::write(cache.join("target/out"), "").expect("write");
        #[cfg(unix)]
        std::os::unix::fs::symlink(dir.path().join("project"), cache.join("cache/link"))
            .expect("link");
        let mut other = spec("rust");
        other.project = ProjectId("p2".into());
        let kept = cache_dir(&caches, &other).expect("cache");
        // Outside a runtime it deletes at once.
        forget(&caches, &ProjectId("p1".into())).expect("forget");
        for bad in ["..", ".", "", "p2/../p2", "/"] {
            forget(&caches, &ProjectId(bad.into())).expect("forget");
        }
        assert!(!caches.join("p1").exists());
        assert!(kept.join("target").is_dir());
        assert!(outside.is_file());
        let left: Vec<_> = std::fs::read_dir(&caches)
            .expect("list")
            .map(|e| e.expect("entry").file_name())
            .collect();
        assert_eq!(left, ["p2"]);
        // Nothing to forget is fine.
        forget(&caches, &ProjectId("p9".into())).expect("forget");
    }

    #[tokio::test]
    async fn the_service_moves_a_forgotten_cache_away_at_once() {
        let dir = tempfile::tempdir().expect("tmp");
        let caches = dir.path().join("lsp");
        let servers =
            Servers::with_known(Arc::new(FakeSandbox::available()), caches.clone(), vec![]);
        cache_dir(&caches, &spec("rust")).expect("cache");
        servers.forget(&ProjectId("p1".into()));
        assert!(!caches.join("p1").exists());
        // A new start gets a new folder while the old one is deleted.
        assert!(cache_dir(&caches, &spec("rust")).expect("cache").is_dir());
    }
}
