//! Pods end to end: the `ovra-pod` binary as pod-init, an identity sandbox
//! (the pod shares this test's network), `sh -c` servers.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use ovra_core::protocol::ErrorCode;
use ovra_core::protocol::projects::ProjectId;
use ovra_core::protocol::servers::{ServerId, ServerReady, ServerStatus};
use ovra_core::services::{
    CommandLine, DevServerSpec, DevServers as _, PodSpec, ProfileNetwork, Result, Sandbox,
    SandboxProfile, SandboxStatus, ServerSink,
};
use ovra_servers::PodServers;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpListener;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

/// Runs every line as it is.
#[derive(Debug)]
struct Identity(bool);

impl Sandbox for Identity {
    fn status(&self) -> SandboxStatus {
        if self.0 {
            SandboxStatus::Available {
                name: "identity".into(),
            }
        } else {
            SandboxStatus::Unavailable {
                reason: "none here".into(),
            }
        }
    }

    fn wrap(&self, profile: &SandboxProfile, line: CommandLine) -> Result<CommandLine> {
        // The pod may write its own folder.
        assert!(profile.write.iter().any(|w| line.args[1] == *w.as_os_str()));
        Ok(line)
    }
}

/// Sends every report as `"<server> <what>"`.
#[derive(Debug)]
struct Sink(UnboundedSender<String>);

impl ServerSink for Sink {
    fn output(&self, id: &ServerId, _: u64, lines: Vec<String>) {
        for line in lines {
            let _ = self.0.send(format!("{} out {line}", id.0));
        }
    }
    fn status(&self, id: &ServerId, generation: u64, status: ServerStatus) {
        let _ = self.0.send(format!("{} #{generation} {status:?}", id.0));
    }
}

struct Setup {
    servers: PodServers,
    data: tempfile::TempDir,
    work: tempfile::TempDir,
    sink: Arc<Sink>,
    reports: UnboundedReceiver<String>,
}

fn setup() -> Setup {
    setup_with(Identity(true))
}

fn setup_with(sandbox: Identity) -> Setup {
    let data = tempfile::tempdir().expect("data dir");
    let work = tempfile::tempdir().expect("work dir");
    let servers = PodServers::new(Arc::new(sandbox), data.path().into())
        .program(env!("CARGO_BIN_EXE_ovra-pod").into());
    let (tx, reports) = unbounded_channel();
    Setup {
        servers,
        data,
        work,
        sink: Arc::new(Sink(tx)),
        reports,
    }
}

impl Setup {
    fn pod(&self) -> PodSpec {
        PodSpec {
            project: ProjectId("p1".into()),
            work_dir: self.work.path().into(),
            profile: SandboxProfile {
                work_dir: self.work.path().into(),
                read: Vec::new(),
                write: Vec::new(),
                expose: Vec::new(),
                forward: Vec::new(),
                network: ProfileNetwork::Blocked,
            },
        }
    }

    fn start(&self, id: &str, script: &str, port: u16, ready: Option<ServerReady>) -> Result<()> {
        let server = DevServerSpec {
            id: ServerId(id.into()),
            generation: 1,
            line: CommandLine {
                program: "sh".into(),
                args: vec!["-c".into(), script.into()],
                env: vec![("GREETING".into(), Some("hi".into()))],
                cwd: self.work.path().into(),
            },
            port,
            ready,
        };
        self.servers.start(&self.pod(), server, self.sink.clone())
    }

    /// Reports up to and including `last`.
    async fn until(&mut self, last: &str) -> Vec<String> {
        let mut seen = Vec::new();
        while let Some(report) = self.reports.recv().await {
            let done = report == last;
            seen.push(report);
            if done {
                return seen;
            }
        }
        panic!("no {last:?} after {seen:?}");
    }
}

impl Setup {
    /// The rest of the first report that starts with `prefix`.
    async fn until_line(&mut self, prefix: &str) -> String {
        while let Some(report) = self.reports.recv().await {
            if let Some(rest) = report.strip_prefix(prefix) {
                return rest.to_owned();
            }
        }
        panic!("no {prefix:?}");
    }
}

fn entries(dir: &Path) -> Vec<PathBuf> {
    let list = std::fs::read_dir(dir).expect("list");
    list.map(|e| e.expect("entry").path()).collect()
}

/// A port nothing listens on.
fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    listener.local_addr().expect("addr").port()
}

#[tokio::test]
async fn a_server_reports_its_output_and_exit_code_and_its_pod_ends_with_it() {
    let mut s = setup();
    let script = "echo \"$GREETING from $(pwd)\"; echo oops >&2; exit 3";
    s.start("srv1", script, free_port(), None).expect("start");
    let work = s.work.path().canonicalize().expect("work");
    assert_eq!(
        s.until("srv1 #1 Exited { code: Some(3) }").await,
        [
            format!("srv1 out hi from {}", work.display()),
            "srv1 out oops".into(),
            "srv1 #1 Exited { code: Some(3) }".into(),
        ]
    );
    // A new server makes a new pod.
    s.start("srv2", "exit 0", free_port(), None).expect("again");
    s.until("srv2 #1 Exited { code: Some(0) }").await;
}

#[tokio::test]
async fn a_listening_port_is_ready_and_reached_through_the_pod() {
    let mut s = setup();
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("addr").port();
    s.start("web", "exec sleep 5", port, None).expect("start");
    s.until("web #1 Ready").await;
    // Another server on that port in this pod clashes.
    let clash = s.start("api", "exit 0", port, None);
    assert_eq!(clash.err().map(|e| e.code), Some(ErrorCode::Conflict));
    let other = std::env::temp_dir();
    assert!(s.servers.connect(other, port).await.is_err());

    let mut conn = s
        .servers
        .connect(s.work.path().into(), port)
        .await
        .expect("connect");
    conn.write_all(b"ping").await.expect("write");
    let mut got = [0; 4];
    // The pod's own probe came first: it connects and closes.
    let mut inside = loop {
        let (mut inside, _) = listener.accept().await.expect("relayed");
        if inside.read_exact(&mut got).await.is_ok() {
            break inside;
        }
    };
    assert_eq!(&got, b"ping");
    inside.write_all(b"pong").await.expect("answer");
    conn.read_exact(&mut got).await.expect("answer read");
    assert_eq!(&got, b"pong");

    s.servers
        .stop(&ServerId("web".into()), Duration::from_secs(5));
    s.until("web #1 Exited { code: None }").await;
    assert!(s.servers.connect(s.work.path().into(), port).await.is_err());
}

#[tokio::test]
async fn ready_by_a_log_line_and_by_an_http_answer() {
    let mut s = setup();
    let log = ServerReady {
        log: Some("ready in".into()),
        http: None,
    };
    s.start(
        "log",
        "echo boot; echo 'ready in 5 ms'; exec sleep 5",
        free_port(),
        Some(log),
    )
    .expect("start");
    s.until("log #1 Ready").await;

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("addr").port();
    tokio::spawn(async move {
        loop {
            let (mut conn, _) = listener.accept().await.expect("accept");
            let mut head = [0; 64];
            // The pod's own probe connects and closes without a request.
            if conn.read(&mut head).await.unwrap_or(0) > 0 {
                let _ = conn.write_all(b"HTTP/1.1 204 No Content\r\n\r\n").await;
            }
        }
    });
    let http = ServerReady {
        log: None,
        http: Some("/health".into()),
    };
    s.start("http", "exec sleep 5", port, Some(http))
        .expect("start");
    s.until("http #1 Ready").await;
    // One pod for both, until the last ends.
    assert_eq!(entries(s.data.path()).len(), 1);
    s.servers
        .stop(&ServerId("log".into()), Duration::from_secs(5));
    s.until("log #1 Exited { code: None }").await;
    s.servers
        .stop(&ServerId("http".into()), Duration::from_secs(5));
    s.until("http #1 Exited { code: None }").await;
}

#[tokio::test]
async fn a_server_that_exits_on_term_keeps_its_code() {
    let mut s = setup();
    let script = "trap 'exit 7' TERM; echo up; sleep 5 & wait";
    s.start("srv", script, free_port(), None).expect("start");
    s.until("srv out up").await;
    s.servers
        .stop(&ServerId("srv".into()), Duration::from_secs(5));
    s.until("srv #1 Exited { code: Some(7) }").await;
}

#[tokio::test(start_paused = true)]
async fn a_server_that_ignores_term_is_killed_after_the_grace() {
    let mut s = setup();
    let script = "trap '' TERM; echo up; sleep 5 & wait";
    s.start("srv", script, free_port(), None).expect("start");
    s.until("srv out up").await;
    s.servers
        .stop(&ServerId("srv".into()), Duration::from_secs(60));
    s.until("srv #1 Exited { code: None }").await;
}

#[tokio::test]
async fn dropping_the_servers_ends_their_pods() {
    let mut s = setup();
    s.start("srv", "echo $$; exec sleep 5", free_port(), None)
        .expect("start");
    let pid = s.until_line("srv out ").await;
    let data = s.data.path().to_owned();
    assert_eq!(entries(&data).len(), 1);
    drop(s.servers);
    assert_eq!(entries(&data), Vec::<PathBuf>::new());
    // Its pod-init kills the server once its control closes; what is
    // left is at most a zombie.
    let stat = format!("/proc/{pid}/stat");
    while std::fs::read_to_string(&stat).is_ok_and(|s| !s.contains(") Z ")) {
        tokio::task::yield_now().await;
    }
}

#[tokio::test]
async fn no_server_may_take_the_sandbox_bridge_port() {
    let s = setup();
    let started = s.start("srv", "exit 0", 3128, None);
    assert_eq!(started.err().map(|e| e.code), Some(ErrorCode::BadRequest));
    assert_eq!(entries(s.data.path()), Vec::<PathBuf>::new());
}

#[tokio::test]
async fn nothing_runs_without_a_sandbox() {
    let s = setup_with(Identity(false));
    assert!(matches!(
        s.servers.status(),
        SandboxStatus::Unavailable { .. }
    ));
    let started = s.start("srv", "exit 0", free_port(), None);
    assert_eq!(started.err().map(|e| e.code), Some(ErrorCode::Unavailable));
    assert_eq!(entries(s.data.path()), Vec::<PathBuf>::new());
}
