use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use ovra_core::Core;
use ovra_transport::stdio::{Hello, QUIT, REPLACED};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use super::{IDLE_EXIT, Private, bind, cut_log, may_end, open_log, same_uid, serve_core};

fn ok<T, E: std::fmt::Debug>(r: Result<T, E>) -> T {
    r.unwrap_or_else(|e| panic!("{e:?}"))
}

fn hello() -> Hello {
    Hello {
        version: "1.2.3".into(),
        protocol: 1,
        os: "linux".into(),
        no_sandbox: None,
        started_ms: Some(5),
    }
}

#[test]
fn a_private_folder_is_made_0700_and_a_symlink_is_refused() {
    let dir = ok(tempfile::tempdir());
    let data = dir.path().join("data");
    ok(std::fs::create_dir(&data));
    ok(std::fs::set_permissions(
        &data,
        std::fs::Permissions::from_mode(0o755),
    ));
    let private = ok(Private::open(&data));
    let mode = ok(std::fs::metadata(&data)).permissions().mode() & 0o777;
    assert_eq!(mode, 0o700);
    let run = ok(private.child_run());
    drop(run);
    assert_eq!(
        ok(std::fs::metadata(data.join("run"))).permissions().mode() & 0o777,
        0o700
    );

    let elsewhere = dir.path().join("elsewhere");
    ok(std::fs::create_dir(&elsewhere));
    let link = dir.path().join("link");
    ok(std::os::unix::fs::symlink(&elsewhere, &link));
    assert!(Private::open(&link).is_err());
    // A planted `run` symlink is refused too.
    let other = dir.path().join("other");
    ok(std::fs::create_dir(&other));
    ok(std::os::unix::fs::symlink(&elsewhere, other.join("run")));
    assert!(ok(Private::open(&other)).child_run().is_err());
}

#[tokio::test]
async fn a_leftover_socket_is_replaced_and_anything_else_refused() {
    let dir = ok(tempfile::tempdir());
    let run = ok(ok(Private::open(dir.path())).child_run());
    let first = ok(bind(&run));
    drop(first);
    // Left behind with nobody listening.
    assert!(dir.path().join("run/core.sock").exists());
    let again = ok(bind(&run));
    let mode = ok(std::fs::symlink_metadata(dir.path().join("run/core.sock")))
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
    drop(again);
    ok(std::fs::remove_file(dir.path().join("run/core.sock")));
    ok(std::fs::write(
        dir.path().join("run/core.sock"),
        "not a socket",
    ));
    assert!(bind(&run).is_err());
}

#[test]
fn a_core_alone_ends_only_after_the_quiet_while_with_nothing_running() {
    let now = tokio::time::Instant::now();
    let long_ago = now.checked_sub(IDLE_EXIT).unwrap_or(now);
    assert!(may_end(Some(long_ago), now, false));
    assert!(!may_end(Some(long_ago), now, true));
    assert!(!may_end(Some(now), now, false));
    assert!(!may_end(None, now, false));
}

#[test]
fn the_log_keeps_its_last_megabyte() {
    let dir = ok(tempfile::tempdir());
    let path = dir.path().join("core.log");
    let mut text = vec![b'a'; 1024 * 1024];
    text.extend_from_slice(b"the end");
    ok(std::fs::write(&path, &text));
    let file = ok(std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path));
    ok(cut_log(&file));
    let kept = ok(std::fs::read(&path));
    assert_eq!(kept.len(), 1024 * 1024);
    assert!(kept.ends_with(b"the end"));
}

struct Client {
    write: tokio::net::unix::OwnedWriteHalf,
    lines: tokio::io::Lines<BufReader<tokio::net::unix::OwnedReadHalf>>,
}

impl Client {
    async fn connect(path: &std::path::Path) -> Self {
        let (read, write) = ok(UnixStream::connect(path).await).into_split();
        Self {
            write,
            lines: BufReader::new(read).lines(),
        }
    }

    async fn line(&mut self) -> String {
        let next = tokio::time::timeout(Duration::from_secs(2), self.lines.next_line()).await;
        ok(ok(next)).unwrap_or_default()
    }

    async fn send(&mut self, line: &str) {
        ok(self.write.write_all(format!("{line}\n").as_bytes()).await);
    }

    /// Reads up to the first line that is not an event.
    async fn answer(&mut self) -> String {
        loop {
            let line = self.line().await;
            if !line.starts_with(r#"{"event""#) {
                return line;
            }
        }
    }
}

#[tokio::test]
async fn a_new_client_takes_over_only_with_its_first_line_and_quit_ends_the_core() {
    let dir = ok(tempfile::tempdir());
    let run = ok(ok(Private::open(dir.path())).child_run());
    let listener = ok(bind(&run));
    let socket = dir.path().join("run/core.sock");
    let core = Core::builder("test").build();
    let served = tokio::spawn(serve_core(core, listener, hello()));

    let mut first = Client::connect(&socket).await;
    assert!(first.line().await.contains(r#""started_ms":5"#));
    assert!(first.line().await.contains("info.ready"));
    first.send(r#"{"ping":{}}"#).await;

    // One that refuses the hello and leaves disturbs nobody.
    let mut passing = Client::connect(&socket).await;
    assert!(passing.line().await.starts_with(r#"{"hello""#));
    drop(passing);
    first.send(r#"{"id":1,"method":"projects.list"}"#).await;
    assert!(first.answer().await.starts_with(r#"{"id":1,"#));

    let mut second = Client::connect(&socket).await;
    assert!(second.line().await.starts_with(r#"{"hello""#));
    assert!(second.line().await.contains("info.ready"));
    // It missed what happened before: it reloads.
    assert!(second.line().await.contains("info.resync"));
    second.send(r#"{"ping":{}}"#).await;
    assert_eq!(first.answer().await, REPLACED);

    second.send(QUIT).await;
    assert!(ok(tokio::time::timeout(Duration::from_secs(2), served).await).is_ok());
}

#[test]
fn only_a_peer_of_the_cores_own_user_is_served() {
    assert!(same_uid(1000, 1000));
    assert!(!same_uid(1001, 1000));
    assert!(!same_uid(0, 1000));
    assert!(!same_uid(1000, 0));
}

#[test]
fn the_log_is_this_users_own_file_and_nothing_planted() {
    let dir = ok(tempfile::tempdir());
    let data = ok(Private::open(dir.path()));
    let path = dir.path().join("core.log");
    ok(std::fs::write(&path, "old"));
    ok(std::fs::set_permissions(
        &path,
        std::fs::Permissions::from_mode(0o644),
    ));
    drop(ok(open_log(&data)));
    assert_eq!(
        ok(std::fs::metadata(&path)).permissions().mode() & 0o777,
        0o600
    );

    ok(std::fs::remove_file(&path));
    let elsewhere = dir.path().join("elsewhere");
    ok(std::fs::write(&elsewhere, "mine"));
    ok(std::os::unix::fs::symlink(&elsewhere, &path));
    assert!(open_log(&data).is_err());

    ok(std::fs::remove_file(&path));
    ok(rustix::fs::mknodat(
        rustix::fs::CWD,
        &path,
        rustix::fs::FileType::Fifo,
        rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
        0,
    ));
    assert!(open_log(&data).is_err());
}

#[tokio::test(start_paused = true)]
async fn a_core_nobody_attaches_to_ends_after_the_quiet_while() {
    let dir = ok(tempfile::tempdir());
    let run = ok(ok(Private::open(dir.path())).child_run());
    let listener = ok(bind(&run));
    let served = tokio::spawn(serve_core(Core::builder("test").build(), listener, hello()));
    tokio::time::sleep(IDLE_EXIT - Duration::from_secs(60)).await;
    assert!(!served.is_finished());
    tokio::time::sleep(Duration::from_secs(120)).await;
    assert!(served.await.is_ok());
}
