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
                gaps: Vec::new(),
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

/// The pods' folder where `servers_dir` puts it without a runtime folder,
/// off the paths servers write.
fn setup_with(sandbox: Identity) -> Setup {
    setup_in(sandbox, Path::new("/var/tmp"))
}

fn setup_in(sandbox: Identity, pods: &Path) -> Setup {
    let data = tempfile::tempdir_in(pods).expect("data dir");
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
                kind: ovra_core::services::ProfileKind::Pod,
                claude_dir: None,
                project: None,
                work_env: None,
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
            port,
            ready,
            ..self.spec(id, script)
        };
        self.servers.start(&self.pod(), server, self.sink.clone())
    }

    fn spec(&self, id: &str, script: &str) -> DevServerSpec {
        DevServerSpec {
            id: ServerId(id.into()),
            generation: 1,
            line: CommandLine {
                program: "sh".into(),
                args: vec!["-c".into(), script.into()],
                env: vec![("GREETING".into(), Some("hi".into()))],
                cwd: self.work.path().into(),
            },
            port: free_port(),
            ready: None,
        }
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
async fn waiting_for_a_pod_lasts_until_its_process_ended() {
    let mut s = setup();
    let work = s.work.path().to_owned();
    // No pod runs: nothing to wait for.
    s.servers.pod_ended(work.clone()).await;
    s.start("srv", "echo up; exec sleep 5", free_port(), None)
        .expect("start");
    s.until("srv out up").await;
    let data = s.data.path().to_owned();
    assert_eq!(entries(&data).len(), 1);
    s.servers
        .stop(&ServerId("srv".into()), Duration::from_secs(5));
    s.servers.pod_ended(work).await;
    // Its folder goes last, once its process has ended.
    assert_eq!(entries(&data), Vec::<PathBuf>::new());
}

#[tokio::test]
async fn a_pod_that_never_listens_ends_with_a_kill() {
    let mut s = setup();
    let script = s.work.path().join("hang.sh");
    std::fs::write(&script, "#!/bin/sh\nexec sleep 30\n").expect("script");
    let mode = std::os::unix::fs::PermissionsExt::from_mode(0o755);
    std::fs::set_permissions(&script, mode).expect("mode");
    let data = s.data.path().to_owned();
    let servers = PodServers::new(Arc::new(Identity(true)), data.clone()).program(script);
    servers
        .start(&s.pod(), s.spec("srv", "exit 0"), s.sink.clone())
        .expect("start");
    servers.stop(&ServerId("srv".into()), Duration::ZERO);
    s.until("srv #1 Failed { reason: \"the pod was stopped before it started\" }")
        .await;
    servers.pod_ended(s.work.path().into()).await;
    assert_eq!(entries(&data), Vec::<PathBuf>::new());
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

#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_server_cannot_make_remove_or_rename_entries_in_its_pod_folder() {
    let mut s = setup();
    let port = free_port();
    let script = format!(
        "pod=$(echo {data}/*)
        try() {{ if (eval \"$2\") 2>/dev/null; then echo \"$1: yes\"; else echo \"$1: no\"; fi; }}
        try 'make a file' \": > $pod/x\"
        try 'make a link' \"ln -s /var/run/docker.sock $pod/l\"
        try 'make a folder' \"mkdir $pod/d\"
        try 'rename its socket' \"mv $pod/{port}.sock $pod/y\"
        try 'remove its socket' \"rm $pod/{port}.sock\"
        try 'write its work dir' ': > made'
        try 'make a temp file' ': > \"${{TMPDIR:-/tmp}}/t$$\" && rm \"${{TMPDIR:-/tmp}}/t$$\"'
        try 'make a file in /tmp' ': > /tmp/ovra-pods-$$ && rm /tmp/ovra-pods-$$'",
        data = s.data.path().display(),
    );
    s.start("srv", &script, port, None).expect("start");
    let seen = s.until("srv #1 Exited { code: Some(0) }").await;
    if seen.contains(&"srv out make a file: yes".to_owned()) && std::env::var_os("CI").is_none() {
        eprintln!("skipped: this kernel has no Landlock");
        return;
    }
    assert_eq!(
        seen,
        [
            "srv out make a file: no",
            "srv out make a link: no",
            "srv out make a folder: no",
            "srv out rename its socket: no",
            "srv out remove its socket: no",
            "srv out write its work dir: yes",
            "srv out make a temp file: yes",
            "srv out make a file in /tmp: yes",
            "srv #1 Exited { code: Some(0) }",
        ]
    );
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_pod_in_the_temp_folder_gives_its_servers_a_temp_folder_of_their_own() {
    let mut s = setup_in(Identity(true), &std::env::temp_dir());
    let script = "case \"$TMPDIR\" in */ovra-pod-tmp-*) echo own;; *) echo shared;; esac
        echo \"$TMPDIR\"; : > \"$TMPDIR/x\" && echo wrote";
    s.start("srv", script, free_port(), None).expect("start");
    let seen = s.until("srv #1 Exited { code: Some(0) }").await;
    let own = seen[1].strip_prefix("srv out ").map(PathBuf::from);
    assert_eq!(
        (seen[0].as_str(), &seen[2..]),
        (
            "srv out own",
            &["srv out wrote", "srv #1 Exited { code: Some(0) }"].map(String::from)[..]
        )
    );
    // Pod-init removes it once its last server ended.
    let own = own.expect("its TMPDIR");
    while own.exists() {
        tokio::task::yield_now().await;
    }
}

#[test]
fn the_pods_folder_is_private_and_off_the_temp_folder() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = ovra_servers::servers_dir().expect("servers dir");
    assert!(!dir.starts_with(std::env::temp_dir()) && !dir.starts_with("/tmp"));
    assert!(dir.as_os_str().len() + "/0123456789abcdef/control.sock".len() < 100);
    let mode = std::fs::metadata(&dir).expect("made").permissions().mode();
    std::fs::remove_dir(&dir).expect("empty");
    assert_eq!(mode & 0o777, 0o700);
}

#[tokio::test]
async fn a_pods_folder_too_long_for_its_sockets_is_refused() {
    let s = setup();
    let long = s.data.path().join("a".repeat(80));
    std::fs::create_dir(&long).expect("long folder");
    let servers = PodServers::new(Arc::new(Identity(true)), long.clone())
        .program(env!("CARGO_BIN_EXE_ovra-pod").into());
    let started = servers.start(&s.pod(), s.spec("srv", "exit 0"), s.sink.clone());
    let error = started.expect_err("too long");
    assert!(error.message.contains("too long"), "{}", error.message);
    assert_eq!(entries(&long), Vec::<PathBuf>::new());
}
