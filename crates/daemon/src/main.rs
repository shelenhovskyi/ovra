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
use ovra_core::services::AgentRuntime;
use ovra_fs_index::FsIndex;
use ovra_mcp_server::IdeServer;
use ovra_opener::SystemOpener;
use ovra_pty::Pty;
use ovra_secrets::KeychainSecrets;
use ovra_store::Store;
use ovra_transport::ws::{self, Access};
use ovra_worktree::GitWorktrees;
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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
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
    let core = Core::builder("headless")
        .projects(store.clone())
        .sessions(store)
        .fs(Arc::new(FsIndex::new()))
        .opener(Arc::new(SystemOpener::new()))
        .worktrees(Arc::new(GitWorktrees::new()))
        .worktrees_dir(args.data_dir.join("worktrees"))
        .ide_tools(Arc::new(IdeServer::new()))
        .terminals(Arc::new(Pty::new()))
        .agents(agents(&args))
        .secrets(Arc::new(KeychainSecrets::new(&args.data_dir)))
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
fn agents(_: &Args) -> Arc<dyn AgentRuntime> {
    Arc::new(ClaudeCli::new(None, ovra_sandbox::for_this_os()))
}

/// The fake agent asks to run a command and to edit a file, then waits for
/// replies; every reply gets one more command to approve. A task's
/// coordinator and workers play their own scripts.
#[cfg(feature = "fake-agent")]
fn agents(args: &Args) -> Arc<dyn AgentRuntime> {
    use ovra_core::fakes::{ScriptedAgent, Step};
    if !args.fake_agent {
        return Arc::new(ClaudeCli::new(None, ovra_sandbox::for_this_os()));
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
        Step::Say("I will list the files, then fix the typo in README.md."),
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
    // (after an approval) and asks for review.
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
    let worker = vec![vec![
        Step::Use {
            tool: "Write",
            input: serde_json::json!({ "file_path": "NOTES.md", "content": "Notes from a worker.\n" }),
        },
        Step::Use {
            tool: "mcp__ovra__request_review",
            input: serde_json::json!({ "summary": "Wrote NOTES.md." }),
        },
    ]];
    let script = ScriptedAgent::new(turns)
        .for_role(ovra_core::services::ToolRole::Coordinator, coordinator)
        .for_role(ovra_core::services::ToolRole::Worker, worker)
        .writes_files(|path, text| std::fs::write(path, text));
    Arc::new(FakeAgent {
        script,
        claude: ClaudeCli::new(None, ovra_sandbox::for_this_os()),
    })
}

/// Plays the script, is always set up, and reads project configuration
/// like the real runtime so the trust flow can be tried.
#[cfg(feature = "fake-agent")]
#[derive(Debug)]
struct FakeAgent {
    script: ovra_core::fakes::ScriptedAgent,
    claude: ClaudeCli,
}

#[cfg(feature = "fake-agent")]
impl AgentRuntime for FakeAgent {
    fn start(
        &self,
        spec: ovra_core::services::AgentSpec,
        host: ovra_core::services::AgentHost,
    ) -> ovra_core::services::Result<Box<dyn ovra_core::services::RunningAgent>> {
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

fn new_token() -> String {
    let bytes: [u8; 16] = rand::rng().random();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
