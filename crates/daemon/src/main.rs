//! `ovra-daemon`: runs the core headless and serves the UI to a browser.
//!
//! On start it prints one line, `OVRA_URL=<url>`, that opens the UI with the
//! per-launch token. Tests and scripts read that line.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use clap::Parser;
use ovra_core::Core;
use ovra_fs_index::FsIndex;
use ovra_opener::SystemOpener;
use ovra_store::Store;
use ovra_transport::ws::{self, Access};
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
    let core = Core::builder("headless")
        .projects(Arc::new(Store::open(&args.data_dir)?))
        .fs(Arc::new(FsIndex::new()))
        .opener(Arc::new(SystemOpener::new()))
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

fn new_token() -> String {
    let bytes: [u8; 16] = rand::rng().random();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
