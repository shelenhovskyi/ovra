//! `ovra-daemon`: runs the core headless and serves the UI to a browser.
//!
//! On start it prints one line, `OVRA_URL=<url>`, that opens the UI with the
//! per-launch token. Tests and scripts read that line.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

#[cfg(target_os = "linux")]
mod attach;
#[cfg(feature = "fake-agent")]
mod fake_ssh;

use anyhow::Context;
use clap::Parser;
use ovra_agent_runtime::ClaudeCli;
use ovra_core::Core;
use ovra_core::services::{
    AgentRuntime, Browser, DevServers, Forge, KeepAwake, LanguageServers, RemoteAccess,
    ResourceMonitor, Sandbox, SandboxStatus, Secrets,
};
use ovra_forge::GhForge;
use ovra_fs_index::FsIndex;
use ovra_mcp_server::IdeServer;
use ovra_opener::SystemOpener;
use ovra_pty::Pty;
use ovra_remote::RemoteServer;
use ovra_resources::SysResources;
use ovra_secrets::KeychainSecrets;
use ovra_servers::PodServers;
use ovra_store::Store;
use ovra_transport::ws::{self, Access};
use ovra_worktree::{GitRepo, GitWorktrees};
use rand::Rng;
use tower_http::services::{ServeDir, ServeFile};

#[derive(Debug, Parser)]
#[command(version, about)]
struct Args {
    /// Port on 127.0.0.1. 0 picks a free one.
    #[arg(long, default_value_t = 0)]
    port: u16,
    /// Directory with the built UI (`pnpm --dir ui build`).
    #[arg(long, default_value = "ui/dist")]
    ui_dir: PathBuf,
    /// Where the project list and other state are kept. Default:
    /// `.ovra-data`, or with `--stdio` `$XDG_DATA_HOME/ovra` (else
    /// `~/.local/share/ovra`).
    #[arg(long = "data-dir")]
    data_dir_flag: Option<PathBuf>,
    #[arg(skip)]
    data_dir: PathBuf,
    /// Serves one client over stdin and stdout instead of a port and the UI:
    /// a remote machine reached over SSH (ADR 0016). Nothing runs for
    /// agents without a working sandbox.
    #[arg(long)]
    stdio: bool,
    /// Runs the core of a remote machine in the background, serving one
    /// client at a time on `<data>/run/core.sock` (ADR 0016). Linux only;
    /// `--attach` starts it.
    #[arg(long, conflicts_with_all = ["stdio", "attach"])]
    core: bool,
    /// What `ssh` runs on a remote machine: relays stdin and stdout to the
    /// machine's `--core`, starting it when none runs. Linux only.
    #[arg(long, conflicts_with = "stdio")]
    attach: bool,
    /// With `--attach`: asks the running core to quit first, so this build
    /// starts. Its agents stop.
    #[arg(long, requires = "attach")]
    restart: bool,
    /// Runs a scripted fake agent instead of the user's `claude` (tests and UX reviews).
    #[cfg(feature = "fake-agent")]
    #[arg(long)]
    fake_agent: bool,
    /// Runs dev servers that start nothing and play a preset named by their
    /// command: `ready` gets ready, `fail` fails, anything else exits with 1.
    /// Worktree setup steps (`.ovra/worktree.json`) pass, except `fail`.
    #[cfg(feature = "fake-agent")]
    #[arg(long)]
    fake_servers: bool,
    /// Runs an agent browser that starts nothing: each page shows one
    /// still frame and has one element to pick. Sessions whose task
    /// starts with "Browse" get a fake sandbox behind their netproxy, so
    /// they may open pages.
    #[cfg(feature = "fake-agent")]
    #[arg(long)]
    fake_browser: bool,
    /// Pushes and opens pull requests in memory instead of with the
    /// person's `gh`: `origin` has every local branch of the project, and
    /// each pull request has three checks: one passed, one failed, one
    /// running.
    #[cfg(feature = "fake-agent")]
    #[arg(long)]
    fake_forge: bool,
    /// Serves `/ws/ssh/<id>`, the bridge of a remote window, with this
    /// daemon as `--stdio --fake-agent` in place of `ssh`.
    #[cfg(feature = "fake-agent")]
    #[arg(long)]
    fake_ssh: bool,
}

fn main() -> anyhow::Result<()> {
    // Inside an agent's sandbox this process is the sandbox's helper.
    ovra_sandbox::helper_if_asked();
    // As a server pod, this process is the pod's init.
    ovra_servers::pod_init_if_asked();
    serve()
}

impl Args {
    /// The data folder as an absolute path: worktrees live in it, and the
    /// core checks agents' absolute file paths against them (board claims,
    /// policies), which a relative folder would never match.
    fn absolute(mut self) -> anyhow::Result<Self> {
        let dir = match (self.data_dir_flag.take(), self.remote()) {
            (Some(dir), _) => dir,
            (None, false) => PathBuf::from(".ovra-data"),
            (None, true) => remote_data_dir().context("finding the data directory")?,
        };
        self.data_dir = std::path::absolute(dir).context("resolving --data-dir")?;
        Ok(self)
    }
}

impl Args {
    /// Whether this is a remote machine's side (ADR 0016): no port, no UI,
    /// no phone access, opener or keeping awake, and the required sandbox.
    fn remote(&self) -> bool {
        self.stdio || self.core || self.attach
    }
}

/// `$XDG_DATA_HOME/ovra`, else `~/.local/share/ovra`.
fn remote_data_dir() -> Option<PathBuf> {
    let xdg = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute());
    let base = xdg.or_else(|| {
        let home = PathBuf::from(std::env::var_os("HOME")?);
        Some(home.join(".local").join("share"))
    })?;
    Some(base.join("ovra"))
}

/// The sandbox agents, language servers, dev servers and the browser run
/// in. With `--stdio` it refuses to run anything when the machine has no
/// working sandbox, instead of running it unsandboxed.
fn services_sandbox(args: &Args) -> Arc<dyn Sandbox> {
    services_sandbox_from(ovra_sandbox::for_this_os(&args.data_dir), args.remote())
}

fn services_sandbox_from(sandbox: Arc<dyn Sandbox>, stdio: bool) -> Arc<dyn Sandbox> {
    if stdio {
        Arc::new(ovra_sandbox::Required::new(sandbox))
    } else {
        sandbox
    }
}

#[tokio::main]
async fn serve() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        .init();
    let args = Args::parse().absolute()?;
    if args.stdio {
        return serve_stdio(&args).await;
    }
    if args.attach {
        return attach(&args).await;
    }
    if args.core {
        return serve_core(&args).await;
    }

    // Loopback only: headless mode must never be reachable from the network.
    let listener =
        tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, args.port)))
            .await
            .context("binding 127.0.0.1")?;
    let addr = listener.local_addr()?;

    let token = new_token();
    let allowed_origins = vec![
        format!("http://{addr}"),
        format!("http://localhost:{}", addr.port()),
    ];

    let index = args.ui_dir.join("index.html");
    if !index.exists() {
        tracing::warn!(path = %index.display(), "UI not built; only /ws will work");
    }
    let static_files = ServeDir::new(&args.ui_dir).fallback(ServeFile::new(index));
    std::fs::create_dir_all(&args.data_dir).context("creating the data directory")?;
    // Dropped last, after the core and its pods.
    let (dev_servers, _servers_dir) = dev_servers(&args)?;
    let (remote, restore) = phone_access(&args);
    let core = services(&args, Core::builder("headless"), dev_servers)?
        .opener(Arc::new(SystemOpener::new()))
        .keep_awake(keep_awake(&args))
        .remote(remote)
        .build();
    if let Some(server) = restore {
        server.restore(core.clone());
    }
    let access = Access {
        token: token.clone(),
        allowed_origins,
    };
    let app = ws::router(core, access.clone())
        .merge(fake_ssh(&args, access))
        .fallback_service(static_files);

    println!("OVRA_URL=http://{addr}/?token={token}");
    tracing::info!(%addr, "ovra-daemon listening");

    // Without TCP_NODELAY a reply that follows an event frame waits for the
    // browser's delayed ACK, about 40 ms on Linux.
    let listener = axum::serve::ListenerExt::tap_io(listener, |tcp| {
        if let Err(error) = tcp.set_nodelay(true) {
            tracing::debug!(%error, "TCP_NODELAY");
        }
    });
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

/// The services headless mode and `--stdio` share: everything but the
/// opener, phone access and keeping the computer awake, which a remote
/// machine never gets.
fn services(
    args: &Args,
    builder: ovra_core::CoreBuilder,
    dev_servers: Arc<dyn DevServers>,
) -> anyhow::Result<ovra_core::CoreBuilder> {
    let store = Arc::new(Store::open(&args.data_dir)?);
    let secrets: Arc<dyn Secrets> = Arc::new(KeychainSecrets::new(&args.data_dir));
    Ok(builder
        .secrets(secrets.clone())
        .projects(store.clone())
        .sessions(store.clone())
        .audit(store)
        .fs(Arc::new(FsIndex::new()))
        .worktrees(Arc::new(GitWorktrees::new()))
        .git(Arc::new(GitRepo::new()))
        .worktrees_dir(args.data_dir.join("worktrees"))
        .ide_tools(Arc::new(IdeServer::new()))
        .terminals(Arc::new(Pty::new()))
        .netproxy(Arc::new(ovra_netproxy::NetProxies::from_env(secrets)?))
        .lsp(language_servers(args))
        .agents(agents(args))
        .dev_servers(dev_servers)
        .browser(agent_browser(args))
        .forge(forge(args))
        .resources(resources(args))
        .sandbox(ovra_sandbox::for_this_os(&args.data_dir)))
}

/// `--stdio`: one client, the laptop's Ovra over the person's `ssh`
/// (ADR 0016, `docs/specs/ssh-remotes.md`). No port and no UI. Sessions
/// stop when the client goes or stays silent for a minute.
async fn serve_stdio(args: &Args) -> anyhow::Result<()> {
    private_dir(&args.data_dir)?;
    let Some(lock) = lock_data_dir(&args.data_dir, 40).await? else {
        // Another `--stdio` core still uses this data (a laptop that went
        // away less than a minute ago).
        println!("{}", ovra_transport::stdio::BUSY);
        anyhow::bail!(
            "another ovra-daemon --stdio uses {}",
            args.data_dir.display()
        );
    };
    let no_sandbox = match ovra_sandbox::for_this_os(&args.data_dir).status() {
        SandboxStatus::Available { .. } => None,
        SandboxStatus::Unavailable { reason } => Some(reason),
    };
    if let Some(reason) = &no_sandbox {
        tracing::warn!(%reason, "no sandbox: nothing runs for agents on this machine");
    }
    // Dropped last, after the core and its pods.
    let (dev_servers, _servers_dir) = dev_servers(args)?;
    let core = services(args, Core::builder("remote"), dev_servers)?.build();
    let hello = ovra_transport::stdio::Hello {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        protocol: ovra_transport::stdio::PROTOCOL,
        os: std::env::consts::OS.to_owned(),
        no_sandbox,
        started_ms: None,
    };
    let input = tokio::io::BufReader::new(tokio::io::stdin());
    let ended = ovra_transport::stdio::serve(core, input, tokio::io::stdout(), &hello).await;
    tracing::info!(?ended, "the client went away; stopping");
    // Held until the process exits: its sessions, pods and servers end
    // only when the runtime shuts down, after this returns.
    std::mem::forget(lock);
    Ok(())
}

#[cfg(target_os = "linux")]
async fn attach(args: &Args) -> anyhow::Result<()> {
    private_dir(&args.data_dir)?;
    attach::attach(&args.data_dir, args.restart).await
}

#[cfg(not(target_os = "linux"))]
async fn attach(_: &Args) -> anyhow::Result<()> {
    anyhow::bail!("--attach runs on Linux only")
}

#[cfg(not(target_os = "linux"))]
async fn serve_core(_: &Args) -> anyhow::Result<()> {
    anyhow::bail!("--core runs on Linux only")
}

/// `--core`: the remote machine's core in the background, wired as
/// `--stdio` is, serving one client at a time on its socket until asked
/// to quit or alone with nothing running (ssh-remotes.md, section 6).
#[cfg(target_os = "linux")]
async fn serve_core(args: &Args) -> anyhow::Result<()> {
    private_dir(&args.data_dir)?;
    let data = attach::Private::open(&args.data_dir)?;
    let Some(lock) = lock_data_dir(&args.data_dir, 1).await? else {
        tracing::warn!("another core uses {}", args.data_dir.display());
        std::process::exit(attach::EXIT_BUSY);
    };
    // Bound before any service starts: the umask it sets is the process's.
    let listener = attach::bind(&data.child_run()?)?;
    let no_sandbox = match ovra_sandbox::for_this_os(&args.data_dir).status() {
        SandboxStatus::Available { .. } => None,
        SandboxStatus::Unavailable { reason } => Some(reason),
    };
    if let Some(reason) = &no_sandbox {
        tracing::warn!(%reason, "no sandbox: nothing runs for agents on this machine");
    }
    let (dev_servers, _servers_dir) = dev_servers(args)?;
    let core = services(args, Core::builder("remote"), dev_servers)?.build();
    let started_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .ok();
    let hello = ovra_transport::stdio::Hello {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        protocol: ovra_transport::stdio::PROTOCOL,
        os: std::env::consts::OS.to_owned(),
        no_sandbox,
        started_ms,
    };
    attach::serve_core(core, listener, hello).await;
    std::mem::forget(lock);
    Ok(())
}

/// Makes `dir` and keeps it to this user alone (0700). A symlink, or a
/// folder this user cannot make private, is refused.
fn private_dir(dir: &std::path::Path) -> anyhow::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(dir).context("creating the data directory")?;
    // Owner compared, mode set on the descriptor, no symlink followed.
    #[cfg(target_os = "linux")]
    attach::Private::open(dir)?;
    let meta = std::fs::symlink_metadata(dir).context("reading the data directory")?;
    anyhow::ensure!(meta.is_dir(), "{} is not a folder", dir.display());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .context("making the data directory private")?;
    }
    Ok(())
}

/// Holds `<dir>/stdio.lock` for this process, trying `tries` times 250 ms
/// apart (40: 10 s) while a core is still stopping; `None` when it stays
/// taken.
async fn lock_data_dir(dir: &std::path::Path, tries: u32) -> anyhow::Result<Option<std::fs::File>> {
    let path = dir.join("stdio.lock");
    if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
        anyhow::bail!("{} is a symlink", path.display());
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .context("opening the lock file")?;
    for attempt in 0..tries {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        match file.try_lock() {
            Ok(()) => return Ok(Some(file)),
            Err(std::fs::TryLockError::WouldBlock) => {}
            Err(std::fs::TryLockError::Error(e)) => {
                return Err(e).context("locking the data directory");
            }
        }
    }
    Ok(None)
}

/// Phone access (ADR 0014), serving the mobile app from the same build,
/// and the server to turn back on once the core exists.
fn real_phone_access(args: &Args) -> (Arc<dyn RemoteAccess>, Option<Arc<RemoteServer>>) {
    let server = Arc::new(RemoteServer::new(
        &args.data_dir.join("remote"),
        ovra_remote::assets_from_dir(args.ui_dir.clone()),
    ));
    (server.clone(), Some(server))
}

#[cfg(not(feature = "fake-agent"))]
fn phone_access(args: &Args) -> (Arc<dyn RemoteAccess>, Option<Arc<RemoteServer>>) {
    real_phone_access(args)
}

/// With the fake agent, phone access that listens nowhere.
#[cfg(feature = "fake-agent")]
fn phone_access(args: &Args) -> (Arc<dyn RemoteAccess>, Option<Arc<RemoteServer>>) {
    if args.fake_agent {
        return (Arc::new(ovra_core::fakes::FakeRemote::default()), None);
    }
    real_phone_access(args)
}

#[cfg(not(feature = "fake-agent"))]
fn forge(_: &Args) -> Arc<dyn Forge> {
    Arc::new(GhForge::new())
}

/// With the fake agent, `--fake-forge` never runs `gh` or pushes.
#[cfg(feature = "fake-agent")]
fn forge(args: &Args) -> Arc<dyn Forge> {
    if !args.fake_forge {
        return Arc::new(GhForge::new());
    }
    tracing::warn!("using the fake forge");
    let fake = FakeForge::default();
    if let Ok(mut issues) = fake.0.issues.lock() {
        let issue = |number: u64, title: &str, text: &str| ovra_core::protocol::pr::Issue {
            number,
            title: title.to_owned(),
            url: format!("https://github.com/o/r/issues/{number}"),
            text: text.to_owned(),
        };
        issues.push(issue(
            12,
            "Login button does nothing",
            "Clicking Log in on the start page does nothing.",
        ));
        issues.push(issue(9, "Dark theme contrast", "Labels are hard to read."));
    }
    Arc::new(fake)
}

/// [`ovra_core::fakes::MemoryForge`] whose `origin` has the project's
/// local branches, and whose pull requests come with checks.
#[cfg(feature = "fake-agent")]
#[derive(Debug, Default)]
struct FakeForge(ovra_core::fakes::MemoryForge);

#[cfg(feature = "fake-agent")]
impl Forge for FakeForge {
    fn status(
        &self,
        root: PathBuf,
        remote: String,
    ) -> ovra_core::services::BoxFuture<
        '_,
        ovra_core::services::Result<ovra_core::protocol::pr::GhStatus>,
    > {
        self.0.status(root, remote)
    }

    fn fetch(
        &self,
        root: PathBuf,
        remote: String,
        branch: String,
    ) -> ovra_core::services::BoxFuture<'_, ovra_core::services::Result<String>> {
        let key = (remote.clone(), branch.clone());
        let known = self.0.branches.lock().map(|b| b.contains_key(&key));
        if known.is_ok_and(|known| !known) {
            // The project's own branch stands in for the remote's.
            let local = std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(["rev-parse", "--verify", "--quiet"])
                .arg(format!("refs/heads/{branch}^{{commit}}"))
                .output();
            if let Ok(out) = local.as_ref()
                && out.status.success()
                && let Ok(mut branches) = self.0.branches.lock()
            {
                let commit = String::from_utf8_lossy(&out.stdout).trim().to_owned();
                branches.insert(key, commit);
            }
        }
        self.0.fetch(root, remote, branch)
    }

    fn push(
        &self,
        root: PathBuf,
        remote: String,
        commit: String,
        branch: String,
        expect: Option<String>,
    ) -> ovra_core::services::BoxFuture<'_, ovra_core::services::Result<()>> {
        self.0.push(root, remote, commit, branch, expect)
    }

    fn create(
        &self,
        root: PathBuf,
        draft: ovra_core::protocol::pr::PrDraft,
    ) -> ovra_core::services::BoxFuture<
        '_,
        ovra_core::services::Result<ovra_core::protocol::pr::PrInfo>,
    > {
        use ovra_core::protocol::pr::{CheckState, PrCheck};
        Box::pin(async move {
            let info = self.0.create(root, draft).await?;
            let check = |name: &str, state, run: u64| PrCheck {
                name: name.to_owned(),
                state,
                url: Some(format!("{}/checks?check={name}", info.url)),
                run: Some(run),
            };
            if let Ok(mut pulls) = self.0.pulls.lock()
                && let Some(pull) = pulls.get_mut(&info.number)
            {
                pull.checks = vec![
                    check("build", CheckState::Passed, 1),
                    check("test", CheckState::Failed, 2),
                    check("lint", CheckState::Pending, 3),
                ];
            }
            if let Ok(mut logs) = self.0.logs.lock() {
                logs.insert(2, "test notes ... FAILED\nerror: 1 test failed".to_owned());
            }
            Ok(info)
        })
    }

    fn find(
        &self,
        root: PathBuf,
        remote: String,
        branch: String,
    ) -> ovra_core::services::BoxFuture<
        '_,
        ovra_core::services::Result<Option<ovra_core::protocol::pr::PrInfo>>,
    > {
        self.0.find(root, remote, branch)
    }

    fn view(
        &self,
        root: PathBuf,
        repo: String,
        number: u64,
    ) -> ovra_core::services::BoxFuture<
        '_,
        ovra_core::services::Result<ovra_core::protocol::pr::PrStatus>,
    > {
        self.0.view(root, repo, number)
    }

    fn failed_log(
        &self,
        root: PathBuf,
        repo: String,
        run: u64,
    ) -> ovra_core::services::BoxFuture<'_, ovra_core::services::Result<String>> {
        self.0.failed_log(root, repo, run)
    }

    fn issues(
        &self,
        root: PathBuf,
        remote: String,
    ) -> ovra_core::services::BoxFuture<
        '_,
        ovra_core::services::Result<Vec<ovra_core::protocol::pr::IssueSummary>>,
    > {
        self.0.issues(root, remote)
    }

    fn issue(
        &self,
        root: PathBuf,
        remote: String,
        number: u64,
    ) -> ovra_core::services::BoxFuture<
        '_,
        ovra_core::services::Result<ovra_core::protocol::pr::Issue>,
    > {
        self.0.issue(root, remote, number)
    }
}

#[cfg(not(feature = "fake-agent"))]
fn keep_awake(_: &Args) -> Arc<dyn KeepAwake> {
    Arc::new(ovra_power::SystemKeepAwake::default())
}

/// With the fake agent, a fake hold: tests must not keep the machine awake.
#[cfg(feature = "fake-agent")]
fn keep_awake(args: &Args) -> Arc<dyn KeepAwake> {
    if args.fake_agent {
        return Arc::new(ovra_core::fakes::FakeKeepAwake::default());
    }
    Arc::new(ovra_power::SystemKeepAwake::default())
}

#[cfg(not(feature = "fake-agent"))]
fn agent_browser(args: &Args) -> Arc<dyn Browser> {
    chromium(args)
}

/// With the fake agent, `--fake-browser` gives a browser that starts nothing.
#[cfg(feature = "fake-agent")]
fn agent_browser(args: &Args) -> Arc<dyn Browser> {
    if !args.fake_browser {
        return chromium(args);
    }
    tracing::warn!("using the fake agent browser");
    Arc::new(fake_browser())
}

/// The user's Chromium found on `PATH`, in the sandbox; never downloaded.
fn chromium(args: &Args) -> Arc<dyn Browser> {
    let sandbox = services_sandbox(args);
    Arc::new(ovra_browser::ChromiumBrowser::new(sandbox, None))
}

#[cfg(not(feature = "fake-agent"))]
fn language_servers(args: &Args) -> Arc<dyn LanguageServers> {
    Arc::new(ovra_lsp::Servers::new(
        services_sandbox(args),
        args.data_dir.join("lsp"),
    ))
}

/// With the fake agent, `rust` files get the fake language server built
/// next to the daemon (`just build-headless`) instead of rust-analyzer,
/// and `typescript` a server that is never installed.
#[cfg(feature = "fake-agent")]
fn language_servers(args: &Args) -> Arc<dyn LanguageServers> {
    let sandbox = services_sandbox(args);
    let caches = args.data_dir.join("lsp");
    if !args.fake_agent {
        return Arc::new(ovra_lsp::Servers::new(sandbox, caches));
    }
    let exe = std::env::current_exe().unwrap_or_default();
    let fake = exe.with_file_name(format!("fake-lsp{}", std::env::consts::EXE_SUFFIX));
    let def = ovra_lsp::ServerDef {
        language: "rust".into(),
        name: "fake-lsp".into(),
        program: fake,
        args: Vec::new(),
        install: "just build-headless".into(),
        env: Vec::new(),
    };
    // One that is never installed, so the missing-server hint can be seen.
    let missing = ovra_lsp::ServerDef {
        language: "typescript".into(),
        name: "fake-ts".into(),
        program: exe.with_file_name("fake-ts-not-installed"),
        args: Vec::new(),
        install: "(not installed in fake mode)".into(),
        env: Vec::new(),
    };
    Arc::new(ovra_lsp::Servers::with_known(
        sandbox,
        caches,
        vec![def, missing],
    ))
}

#[cfg(not(feature = "fake-agent"))]
fn resources(_: &Args) -> Arc<dyn ResourceMonitor> {
    Arc::new(SysResources::new())
}

/// The fake agent's processes do not exist: each of its sessions shows
/// the same made-up usage on a calm machine.
#[cfg(feature = "fake-agent")]
fn resources(args: &Args) -> Arc<dyn ResourceMonitor> {
    use ovra_core::fakes::{FakeResources, fake_pid};
    use ovra_core::protocol::agents::SessionId;
    use ovra_core::protocol::resources::Usage;
    if !args.fake_agent {
        return Arc::new(SysResources::new());
    }
    let fake = FakeResources::default();
    fake.set_system(16 << 30, 9 << 30, 18);
    for n in 1..=50 {
        let usage = Usage {
            memory_bytes: (380 + 20 * u64::from(n % 5)) << 20,
            cpu_percent: 4 + n % 7,
            processes: 3,
        };
        fake.set_tree(fake_pid(&SessionId(format!("s{n}"))), usage);
    }
    Arc::new(fake)
}

/// Claude through its own runtime, every other provider over ACP, each
/// sandboxed for this OS.
fn real_agents(args: &Args) -> Arc<dyn AgentRuntime> {
    let sandbox = || services_sandbox(args);
    Arc::new(ovra_core::runtimes::AgentRuntimes::new(vec![
        Arc::new(ClaudeCli::new(None, sandbox())),
        Arc::new(ovra_acp_client::AcpAgents::new(
            ovra_acp_client::known(),
            sandbox(),
        )),
    ]))
}

#[cfg(not(feature = "fake-agent"))]
fn agents(args: &Args) -> Arc<dyn AgentRuntime> {
    real_agents(args)
}

/// The fake agent asks to run a command and to edit a file, then waits for
/// replies; every reply gets one more command to approve. A task's
/// coordinator and workers play their own scripts.
#[cfg(feature = "fake-agent")]
fn agents(args: &Args) -> Arc<dyn AgentRuntime> {
    use ovra_core::fakes::{ScriptedAgent, Step};
    if !args.fake_agent {
        return real_agents(args);
    }
    tracing::warn!("using the scripted fake agent");
    let reply = || {
        vec![
            Step::Say("Checking the status."),
            Step::Use {
                tool: "Bash",
                input: serde_json::json!({ "command": "git status", "description": "Show status" }),
            },
        ]
    };
    let mut turns = vec![vec![
        Step::Stream("I will list the files, then fix the typo in README.md."),
        Step::Use {
            tool: "Bash",
            input: serde_json::json!({ "command": "ls -la", "description": "List files" }),
        },
        Step::Use {
            tool: "Edit",
            input: serde_json::json!({
                "file_path": "README.md",
                "old_string": "teh",
                "new_string": "the",
            }),
        },
        Step::Say("Done."),
    ]];
    turns.extend(std::iter::repeat_with(reply).take(20));
    // A task: the coordinator starts three workers, and each writes its notes
    // (after an approval) and asks for review; a comment on the review has it
    // rewrite them and ask again.
    let spawn = |name: &str| Step::Use {
        tool: "mcp__ovra__spawn_agent",
        input: serde_json::json!({ "name": name, "task": format!("Write the {name} notes.") }),
    };
    let coordinator = vec![vec![
        Step::Say("I will split this among three workers."),
        spawn("docs"),
        spawn("tests"),
        spawn("lint"),
        Step::Say("Three workers are on it."),
    ]];
    let worker = vec![
        vec![
            Step::Use {
                tool: "Write",
                input: serde_json::json!({ "file_path": "NOTES.md", "content": "Notes from a worker.\n" }),
            },
            Step::Use {
                tool: "mcp__ovra__request_review",
                input: serde_json::json!({ "summary": "Wrote NOTES.md." }),
            },
        ],
        vec![
            Step::Use {
                tool: "Write",
                input: serde_json::json!({ "file_path": "NOTES.md", "content": "Notes reworked after your comment.\n" }),
            },
            Step::Use {
                tool: "mcp__ovra__request_review",
                input: serde_json::json!({ "summary": "Reworked NOTES.md." }),
            },
        ],
    ];
    let script = ScriptedAgent::new(turns)
        .for_role(ovra_core::services::ToolRole::Coordinator, coordinator)
        .for_role(ovra_core::services::ToolRole::Worker, worker)
        .writes_files(|path, text| std::fs::write(path, text));
    // Sessions that browse say one thing, ask nothing, and report a fake
    // sandbox behind their netproxy.
    let browsing = ScriptedAgent::new(vec![vec![Step::Say("I will look at the page.")]]);
    // Sessions for tasks that start with "Sandboxed" run the tests twice in
    // the same fake sandbox, so a rule from the first approval can decide
    // the second.
    let npm_test = || Step::Use {
        tool: "Bash",
        input: serde_json::json!({ "command": "npm test", "description": "Run the tests" }),
    };
    let sandboxed = ScriptedAgent::new(vec![vec![
        npm_test(),
        npm_test(),
        Step::Say("The tests ran twice."),
    ]]);
    for agent in [&browsing, &sandboxed] {
        if let Ok(mut sandbox) = agent.sandbox.lock() {
            *sandbox = Some(ovra_core::protocol::agents::SandboxReport {
                name: "fake sandbox for tests".into(),
                network: ovra_core::protocol::agents::SandboxNetwork::Proxy,
                gaps: vec!["everything: nothing is confined".into()],
            });
        }
    }
    // Sessions that stream are caught mid-sentence until interrupted.
    let streaming = ScriptedAgent::new(vec![vec![Step::Partial(
        "Reading the code first, then I will run the tests",
    )]]);
    // The git panel's agent actions (docs/specs/git-agent-actions.md): each
    // answers as the real agent is asked to, for the e2e repository's files.
    let say = |text: &'static str| ScriptedAgent::new(vec![vec![Step::Say(text)]]);
    let git_actions = vec![
        (
            "Suggest commits",
            say("Two commits.\n```json\n{\"commits\": [\
                 {\"message\": \"docs: greet the world\", \"files\": [\"README.md\"]},\
                 {\"message\": \"chore: add new.txt\", \"files\": [\"new.txt\"]}]}\n```"),
        ),
        (
            "Write a commit message",
            say("```text\nchore: update the staged files\n```"),
        ),
        (
            "Summarize what's new",
            say("- A new file, other.txt, came in.\n- Nothing else changed."),
        ),
        (
            "Explain the conflicts",
            say("Both sides changed the same line; keep the one you want and stage the file."),
        ),
        // An answer that lists numbered items, which the person can hand to
        // agents of their own (ui/src/agents/items.ts).
        (
            "Find bugs",
            say(
                "Found two:\n\n1. **Race** in `watch.rs`\n2. Typo in README.md\n\nWant me to fix them?",
            ),
        ),
        // Each item handed on from that answer ends at once, asking nothing.
        ("Item ", say("Done.")),
        // Reviews' "Explain these changes" (ui/src/inbox/explain.ts).
        (
            "Explain these changes",
            say(
                "Here is what changed.\n```json\n{\"overall\": [\"The agent tidied up the notes so they read better.\"], \
                 \"files\": [{\"path\": \"NOTES.md\", \"explanation\": \"Adds a line about the agent's work.\"}]}\n```",
            ),
        ),
        // A task that writes a README: an answer that ends
        // with the three-line summary (agents/roles/task.md).
        (
            "Write a README.md",
            say(
                "I read the code and wrote the README.\n\nDone: wrote README.md\n\
                 Checked: every command in it ran\nLeft: Nothing",
            ),
        ),
    ];
    // Sessions for tasks that start with "Checkpoint" write one file per
    // turn, so going back to a turn has something to set back.
    let write = |file: &str| Step::Use {
        tool: "Write",
        input: serde_json::json!({ "file_path": file, "content": format!("{file} from the agent\n") }),
    };
    let checkpoints = ScriptedAgent::new(vec![
        vec![write("one.txt"), Step::Say("Wrote one.txt.")],
        vec![write("two.txt"), Step::Say("Wrote two.txt.")],
    ])
    .writes_files(|path, text| std::fs::write(path, text))
    .runs_hooks();
    Arc::new(FakeAgent {
        script,
        git_actions,
        browsing,
        sandboxed,
        streaming,
        checkpoints,
        claude: ClaudeCli::new(None, services_sandbox(args)),
    })
}

/// A grey page with a dark bar, 64x40 (a 1280x800 viewport scaled down).
#[cfg(feature = "fake-agent")]
const FAKE_FRAME: &str = "/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDACgcHiMeGSgjISMtKygwPGRBPDc3PHtYXUlkkYCZlo+AjIqgtObDoKrarYqMyP/L2u71////m8H////6/+b9//j/wAALCAAoAEABAREA/8QAGQABAAIDAAAAAAAAAAAAAAAAAAQGAQMF/8QAIBABAAIBBAIDAAAAAAAAAAAAAAEDAhVSkdEEETRysf/aAAgBAQAAPwDrAAAAACtgLB43xqvpH42AhaXRvs5jo0ujfZzHRpdG+zmOjS6N9nMdJmGEYV44R79YxER7ZAAAf//Z";

#[cfg(feature = "fake-agent")]
fn fake_browser() -> ovra_core::fakes::FakeBrowser {
    let browser = ovra_core::fakes::FakeBrowser::default();
    let bar = serde_json::json!({
        "pick_id": "", "url": "", "selector": "main > button.save", "locator": "button \"Save\"",
        "tag": "button", "text": "Save", "html": "<button class=\"save\">Save</button>",
        "rect": {"x": 160, "y": 320, "width": 640, "height": 200},
        "styles": {"color": "rgb(255, 255, 255)", "background-color": "rgb(60, 60, 60)"},
    });
    if let (Ok(mut element), Ok(mut frame)) = (browser.element.lock(), browser.on_screencast.lock())
    {
        *element = serde_json::from_value(bar).ok();
        *frame = Some(FAKE_FRAME.to_owned());
    }
    browser
}

/// Plays the script, is always set up, and reads project configuration
/// like the real runtime so the trust flow can be tried.
#[cfg(feature = "fake-agent")]
#[derive(Debug)]
struct FakeAgent {
    script: ovra_core::fakes::ScriptedAgent,
    /// For tasks that start with "Browse".
    browsing: ovra_core::fakes::ScriptedAgent,
    /// For tasks that start with "Sandboxed".
    sandboxed: ovra_core::fakes::ScriptedAgent,
    /// For tasks that start with "Stream".
    streaming: ovra_core::fakes::ScriptedAgent,
    /// For the git panel's agent actions, the "Write a README" starter and "Find bugs", by the start of their prompt.
    git_actions: Vec<(&'static str, ovra_core::fakes::ScriptedAgent)>,
    /// For tasks that start with "Checkpoint".
    checkpoints: ovra_core::fakes::ScriptedAgent,
    claude: ClaudeCli,
}

#[cfg(feature = "fake-agent")]
impl AgentRuntime for FakeAgent {
    fn providers(&self) -> Vec<ovra_core::protocol::agents::ProviderInfo> {
        self.claude.providers()
    }

    fn start(
        &self,
        spec: ovra_core::services::AgentSpec,
        host: ovra_core::services::AgentHost,
    ) -> ovra_core::services::Result<Box<dyn ovra_core::services::RunningAgent>> {
        if spec.prompt.starts_with("Browse") {
            return self.browsing.start(spec, host);
        }
        if spec.prompt.starts_with("Sandboxed") {
            return self.sandboxed.start(spec, host);
        }
        if let Some((_, agent)) = self
            .git_actions
            .iter()
            .find(|(start, _)| spec.prompt.starts_with(start))
        {
            return agent.start(spec, host);
        }
        if spec.prompt.starts_with("Stream") {
            return self.streaming.start(spec, host);
        }
        if spec.prompt.starts_with("Checkpoint") {
            return self.checkpoints.start(spec, host);
        }
        self.script.start(spec, host)
    }

    fn project_config(
        &self,
        root: PathBuf,
    ) -> ovra_core::services::BoxFuture<
        '_,
        ovra_core::services::Result<ovra_core::protocol::agents::ProjectAgentConfig>,
    > {
        self.claude.project_config(root)
    }
}

/// The pods' folder (`ovra_servers::servers_dir`), removed when the daemon
/// shuts down; one killed outright leaves it behind.
#[derive(Debug)]
struct ServersDir(Option<PathBuf>);

impl Drop for ServersDir {
    fn drop(&mut self) {
        if let Some(dir) = &self.0 {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

/// Servers in sandbox pods, with the pods' sockets in a new private folder.
fn pod_servers(args: &Args) -> anyhow::Result<(Arc<dyn DevServers>, ServersDir)> {
    let dir = ovra_servers::servers_dir().context("making the folder for server pods")?;
    let servers = PodServers::new(services_sandbox(args), dir.clone());
    Ok((Arc::new(servers), ServersDir(Some(dir))))
}

#[cfg(not(feature = "fake-agent"))]
fn dev_servers(args: &Args) -> anyhow::Result<(Arc<dyn DevServers>, ServersDir)> {
    pod_servers(args)
}

#[cfg(feature = "fake-agent")]
fn dev_servers(args: &Args) -> anyhow::Result<(Arc<dyn DevServers>, ServersDir)> {
    if !args.fake_servers {
        return pod_servers(args);
    }
    tracing::warn!("using fake dev servers");
    Ok((Arc::new(PresetServers::default()), ServersDir(None)))
}

/// [`ovra_core::fakes::FakeDevServers`] that report by themselves, after
/// `start` returned, as the program named in `.ovra/servers.json` says.
#[cfg(feature = "fake-agent")]
#[derive(Debug, Default)]
struct PresetServers(Arc<ovra_core::fakes::FakeDevServers>);

#[cfg(feature = "fake-agent")]
impl ovra_core::services::DevServers for PresetServers {
    fn status(&self) -> ovra_core::services::SandboxStatus {
        self.0.status()
    }

    fn start(
        &self,
        pod: &ovra_core::services::PodSpec,
        server: ovra_core::services::DevServerSpec,
        sink: Arc<dyn ovra_core::services::ServerSink>,
    ) -> ovra_core::services::Result<()> {
        use ovra_core::protocol::servers::ServerStatus;
        let id = server.id.clone();
        let preset = server.line.program.to_string_lossy().into_owned();
        let port = server.port;
        self.0.start(pod, server, sink)?;
        let servers = self.0.clone();
        tokio::spawn(async move {
            tokio::task::yield_now().await;
            servers.output(&id, &["\u{1b}[32mfake\u{1b}[0m starting"]);
            let status = match preset.as_str() {
                "ready" => {
                    let line = format!("ready in 1 ms on port {port}");
                    servers.output(&id, &[line.as_str()]);
                    ServerStatus::Ready
                }
                "fail" => ServerStatus::Failed {
                    reason: format!("port {port} is taken"),
                },
                _ => {
                    servers.output(&id, &["\u{1b}[31merror\u{1b}[0m: no such preset"]);
                    ServerStatus::Exited { code: Some(1) }
                }
            };
            servers.report(&id, status);
        });
        Ok(())
    }

    fn stop(&self, id: &ovra_core::protocol::servers::ServerId, grace: std::time::Duration) {
        self.0.stop(id, grace);
    }

    fn connect(
        &self,
        work_dir: PathBuf,
        port: u16,
    ) -> ovra_core::services::BoxFuture<'_, ovra_core::services::Result<ovra_core::services::PodConn>>
    {
        self.0.connect(work_dir, port)
    }

    fn setup(
        &self,
        _pod: &ovra_core::services::PodSpec,
        steps: Vec<ovra_core::services::SetupStep>,
        sink: Arc<dyn ovra_core::services::SetupSink>,
    ) -> ovra_core::services::BoxFuture<
        '_,
        ovra_core::services::Result<ovra_core::services::SetupEnd>,
    > {
        use ovra_core::services::SetupEnd;
        // Each step says its program; a step whose program is `fail` exits with 1.
        let mut end = SetupEnd::Done;
        for (index, step) in steps.iter().enumerate() {
            let program = step.line.program.to_string_lossy().into_owned();
            sink.step(index);
            sink.output(index, vec![format!("fake {program}")]);
            if program == "fail" {
                end = SetupEnd::Failed {
                    step: index,
                    code: Some(1),
                    reason: None,
                };
                break;
            }
        }
        Box::pin(async move { Ok(end) })
    }

    fn pod_ended(&self, work_dir: PathBuf) -> ovra_core::services::BoxFuture<'_, ()> {
        self.0.pod_ended(work_dir)
    }
}

/// The e2e bridge of remote windows, with `--fake-ssh` only.
#[cfg(feature = "fake-agent")]
fn fake_ssh(args: &Args, access: Access) -> axum::Router {
    if args.fake_ssh {
        fake_ssh::routes(access, &args.data_dir)
    } else {
        axum::Router::new()
    }
}

#[cfg(not(feature = "fake-agent"))]
fn fake_ssh(_: &Args, _: Access) -> axum::Router {
    axum::Router::new()
}

fn new_token() -> String {
    let bytes: [u8; 16] = rand::rng().random();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::Args;

    #[test]
    fn a_remote_core_keeps_its_data_in_the_users_data_folder() {
        let args = Args::parse_from(["ovra-daemon", "--stdio"]).absolute();
        let args = args.unwrap_or_else(|e| panic!("{e}"));
        assert!(args.data_dir.is_absolute());
        assert!(
            args.data_dir.ends_with("ovra"),
            "{}",
            args.data_dir.display()
        );
        let args = Args::parse_from(["ovra-daemon", "--stdio", "--data-dir", "/d"]).absolute();
        assert_eq!(
            args.map(|a| a.data_dir).ok(),
            Some(std::path::PathBuf::from("/d"))
        );
    }

    #[test]
    fn the_core_and_attach_are_wired_as_a_remote_machines_side() {
        for flag in ["--core", "--attach"] {
            let args = Args::parse_from(["ovra-daemon", flag]).absolute();
            let args = args.unwrap_or_else(|e| panic!("{e}"));
            assert!(args.remote(), "{flag}");
            assert!(args.data_dir.ends_with("ovra"), "{flag}");
        }
        assert!(!Args::parse_from(["ovra-daemon"]).remote());
        assert!(Args::try_parse_from(["ovra-daemon", "--core", "--stdio"]).is_err());
        assert!(Args::try_parse_from(["ovra-daemon", "--restart"]).is_err());
    }

    #[test]
    fn a_remote_core_never_runs_agents_unsandboxed() {
        use ovra_core::services::{CommandLine, SandboxStatus};
        let none = std::sync::Arc::new(ovra_sandbox::Unsandboxed);
        let line = CommandLine {
            program: "claude".into(),
            args: Vec::new(),
            env: Vec::new(),
            cwd: "/w".into(),
        };
        let profile = ovra_core::services::SandboxProfile {
            more_work_dirs: Vec::new(),
            kind: ovra_core::services::ProfileKind::Agent,
            agent_state: None,
            project: None,
            work_env: None,
            work_dir: "/w".into(),
            work_dir_read_only: false,
            read: Vec::new(),
            write: Vec::new(),
            expose: Vec::new(),
            forward: Vec::new(),
            network: ovra_core::services::ProfileNetwork::Open,
        };
        let remote = super::services_sandbox_from(none.clone(), true);
        assert!(matches!(remote.status(), SandboxStatus::Available { .. }));
        assert!(remote.wrap(&profile, line.clone()).is_err());
        // Headless mode on the person's own computer keeps today's rule.
        let local = super::services_sandbox_from(none, false);
        assert!(matches!(local.status(), SandboxStatus::Unavailable { .. }));
        assert!(local.wrap(&profile, line).is_ok());
    }

    #[tokio::test]
    async fn a_second_remote_core_finds_the_data_taken() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        super::private_dir(dir.path()).unwrap_or_else(|e| panic!("{e}"));
        let first = super::lock_data_dir(dir.path(), 1).await.ok().flatten();
        assert!(first.is_some());
        let second = super::lock_data_dir(dir.path(), 2).await.ok().flatten();
        assert!(second.is_none());
        drop(first);
        assert!(
            super::lock_data_dir(dir.path(), 1)
                .await
                .ok()
                .flatten()
                .is_some()
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.path()).map(|m| m.permissions().mode() & 0o777);
            assert_eq!(mode.ok(), Some(0o700));
        }
    }

    #[test]
    fn the_default_data_folder_is_made_absolute() {
        let args = Args::parse_from(["ovra-daemon"]).absolute();
        let args = args.unwrap_or_else(|e| panic!("{e}"));
        assert!(args.data_dir.is_absolute(), "{}", args.data_dir.display());
        assert!(args.data_dir.ends_with(".ovra-data"));
    }
}
