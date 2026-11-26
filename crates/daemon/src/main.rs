//! `ovra-daemon`: runs the core headless and serves the UI to a browser.
//!
//! On start it prints one line, `OVRA_URL=<url>`, that opens the UI with the
//! per-launch token. Tests and scripts read that line.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use clap::Parser;
use ovra_agent_runtime::ClaudeCli;
use ovra_core::Core;
use ovra_core::services::{
    AgentRuntime, Browser, DevServers, Forge, KeepAwake, LanguageServers, ResourceMonitor, Secrets,
};
use ovra_forge::GhForge;
use ovra_fs_index::FsIndex;
use ovra_mcp_server::IdeServer;
use ovra_opener::SystemOpener;
use ovra_pty::Pty;
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
    /// Where the project list and other state are kept.
    #[arg(long, default_value = ".ovra-data")]
    data_dir: PathBuf,
    /// Runs a scripted fake agent instead of the user's `claude` (tests and UX reviews).
    #[cfg(feature = "fake-agent")]
    #[arg(long)]
    fake_agent: bool,
    /// Runs dev servers that start nothing and play a preset named by their
    /// command: `ready` gets ready, `fail` fails, anything else exits with 1.
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
}

fn main() -> anyhow::Result<()> {
    // Inside an agent's sandbox this process is the sandbox's helper.
    ovra_sandbox::helper_if_asked();
    // As a server pod, this process is the pod's init.
    ovra_servers::pod_init_if_asked();
    serve()
}

#[tokio::main]
async fn serve() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        .init();
    let args = Args::parse();

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
    let store = Arc::new(Store::open(&args.data_dir)?);
    let secrets: Arc<dyn Secrets> = Arc::new(KeychainSecrets::new(&args.data_dir));
    // Dropped last, after the core and its pods.
    let (dev_servers, _servers_dir) = dev_servers(&args)?;
    let core = Core::builder("headless")
        .secrets(secrets.clone())
        .projects(store.clone())
        .sessions(store.clone())
        .audit(store)
        .fs(Arc::new(FsIndex::new()))
        .opener(Arc::new(SystemOpener::new()))
        .worktrees(Arc::new(GitWorktrees::new()))
        .git(Arc::new(GitRepo::new()))
        .worktrees_dir(args.data_dir.join("worktrees"))
        .ide_tools(Arc::new(IdeServer::new()))
        .terminals(Arc::new(Pty::new()))
        .netproxy(Arc::new(ovra_netproxy::NetProxies::from_env(secrets)?))
        .lsp(language_servers(&args))
        .agents(agents(&args))
        .dev_servers(dev_servers)
        .browser(agent_browser(&args))
        .forge(forge(&args))
        .resources(resources(&args))
        .keep_awake(keep_awake(&args))
        .build();
    let app = ws::router(
        core,
        Access {
            token: token.clone(),
            allowed_origins,
        },
    )
    .fallback_service(static_files);

    println!("OVRA_URL=http://{addr}/?token={token}");
    tracing::info!(%addr, "ovra-daemon listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
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
    Arc::new(FakeForge::default())
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

    fn view(
        &self,
        root: PathBuf,
        number: u64,
    ) -> ovra_core::services::BoxFuture<
        '_,
        ovra_core::services::Result<ovra_core::protocol::pr::PrStatus>,
    > {
        self.0.view(root, number)
    }

    fn failed_log(
        &self,
        root: PathBuf,
        run: u64,
    ) -> ovra_core::services::BoxFuture<'_, ovra_core::services::Result<String>> {
        self.0.failed_log(root, run)
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
    let sandbox = ovra_sandbox::for_this_os(&args.data_dir);
    Arc::new(ovra_browser::ChromiumBrowser::new(sandbox, None))
}

#[cfg(not(feature = "fake-agent"))]
fn language_servers(args: &Args) -> Arc<dyn LanguageServers> {
    Arc::new(ovra_lsp::Servers::new(
        ovra_sandbox::for_this_os(&args.data_dir),
        args.data_dir.join("lsp"),
    ))
}

/// With the fake agent, `rust` files get the fake language server built
/// next to the daemon (`just build-headless`) instead of rust-analyzer,
/// and `typescript` a server that is never installed.
#[cfg(feature = "fake-agent")]
fn language_servers(args: &Args) -> Arc<dyn LanguageServers> {
    let sandbox = ovra_sandbox::for_this_os(&args.data_dir);
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

#[cfg(not(feature = "fake-agent"))]
fn agents(args: &Args) -> Arc<dyn AgentRuntime> {
    Arc::new(ClaudeCli::new(
        None,
        ovra_sandbox::for_this_os(&args.data_dir),
    ))
}

/// The fake agent asks to run a command and to edit a file, then waits for
/// replies; every reply gets one more command to approve. A task's
/// coordinator and workers play their own scripts.
#[cfg(feature = "fake-agent")]
fn agents(args: &Args) -> Arc<dyn AgentRuntime> {
    use ovra_core::fakes::{ScriptedAgent, Step};
    if !args.fake_agent {
        return Arc::new(ClaudeCli::new(
            None,
            ovra_sandbox::for_this_os(&args.data_dir),
        ));
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
        claude: ClaudeCli::new(None, ovra_sandbox::for_this_os(&args.data_dir)),
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
    /// For the git panel's agent actions, by the start of their prompt.
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
    let servers = PodServers::new(ovra_sandbox::for_this_os(&args.data_dir), dir.clone());
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
        pod: &ovra_core::services::PodSpec,
        steps: Vec<ovra_core::services::SetupStep>,
        sink: Arc<dyn ovra_core::services::SetupSink>,
    ) -> ovra_core::services::BoxFuture<
        '_,
        ovra_core::services::Result<ovra_core::services::SetupEnd>,
    > {
        self.0.setup(pod, steps, sink)
    }

    fn pod_ended(&self, work_dir: PathBuf) -> ovra_core::services::BoxFuture<'_, ()> {
        self.0.pod_ended(work_dir)
    }
}

fn new_token() -> String {
    let bytes: [u8; 16] = rand::rng().random();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
