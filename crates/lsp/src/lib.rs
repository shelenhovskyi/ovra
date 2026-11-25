//! Language servers for trusted projects (ADR 0007, `docs/specs/lsp.md`):
//! finds the servers Ovra knows on `PATH`, starts them through the
//! sandbox, and frames their JSON-RPC messages. Trust, URIs and the
//! requests a server makes are the core's (`crates/core/src/lsp/`).
//! Contract: `ovra_core::services::LanguageServers`; fake:
//! `ovra_core::fakes::FakeLanguageServers`.

mod frame;
mod known;
mod process;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ovra_core::protocol::lsp::KnownServer;
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
                SandboxStatus::Unavailable { .. } => (line, None),
            };
            let process = process::spawn(&line, &def.name, host)?;
            Ok(StartedServer {
                process: Box::new(process),
                name: def.name.clone(),
                sandbox,
            })
        })
    }
}

/// `<caches>/<project id>/<language>`, created with the folders the
/// server's environment points at.
fn cache_dir(caches: &Path, spec: &ServerSpec) -> Result<PathBuf> {
    let plain = |s: &str| !s.is_empty() && s != "." && s != ".." && !s.contains(['/', '\\']);
    if !plain(&spec.project.0) || !plain(&spec.language) {
        return Err(ProtocolError::new(
            ErrorCode::BadRequest,
            "not a project and language a cache folder can be named after",
        ));
    }
    let cache = caches.join(&spec.project.0).join(&spec.language);
    for sub in ["target", "cache", "tmp"] {
        std::fs::create_dir_all(cache.join(sub)).map_err(|e| {
            ProtocolError::new(
                ErrorCode::Internal,
                format!("could not create {}: {e}", cache.display()),
            )
        })?;
    }
    Ok(cache)
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
mod tests {
    use ovra_core::fakes::FakeSandbox;
    use ovra_core::protocol::projects::ProjectId;

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
}
