//! The e2e bridge of remote windows (`--fake-ssh`, `fake-agent` builds):
//! `/ws/ssh/<id>` carries frames to a fake remote core and has the token
//! check of `/ws`.
#![cfg(feature = "fake-agent")]

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

fn ok<T, E: std::fmt::Display>(r: Result<T, E>) -> T {
    r.unwrap_or_else(|e| panic!("{e}"))
}

/// A headless daemon, the `http://host:port` and token it serves, and its
/// data folder.
struct Daemon {
    child: Child,
    base: String,
    token: String,
    _data: tempfile::TempDir,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// The next text frame, as JSON.
async fn frame(socket: &mut Socket) -> serde_json::Value {
    loop {
        match socket.next().await {
            Some(Ok(Message::Text(text))) => return ok(serde_json::from_str(&text)),
            Some(Ok(_)) => {}
            other => panic!("{other:?}"),
        }
    }
}

fn start(extra: &[&str]) -> Daemon {
    let data = ok(tempfile::tempdir());
    let mut child = ok(Command::new(env!("CARGO_BIN_EXE_ovra-daemon"))
        .args(["--port", "0", "--fake-agent", "--data-dir"])
        .arg(data.path())
        .arg("--ui-dir")
        .arg(data.path())
        .args(extra)
        .env("RUST_LOG", "error")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn());
    let mut out = BufReader::new(child.stdout.take().unwrap_or_else(|| panic!("no stdout")));
    let mut line = String::new();
    ok(out.read_line(&mut line));
    let url = line
        .trim()
        .strip_prefix("OVRA_URL=")
        .unwrap_or_default()
        .to_owned();
    let (base, token) = url
        .split_once("/?token=")
        .unwrap_or_else(|| panic!("{line}"));
    Daemon {
        child,
        base: base.to_owned(),
        token: token.to_owned(),
        _data: data,
    }
}

fn ws(daemon: &Daemon, path: &str, token: &str) -> String {
    format!(
        "{}{path}?token={token}",
        daemon.base.replace("http://", "ws://")
    )
}

#[tokio::test]
async fn a_remote_window_reaches_its_own_core_with_the_token_only() {
    let daemon = start(&["--fake-ssh"]);
    let refused = tokio_tungstenite::connect_async(ws(&daemon, "/ws/ssh/box1", "wrong")).await;
    assert!(refused.is_err());

    let (mut socket, _) =
        ok(tokio_tungstenite::connect_async(ws(&daemon, "/ws/ssh/box1", &daemon.token)).await);
    // The hello stays with the bridge; the window starts at `info.ready`.
    assert_eq!(frame(&mut socket).await["event"]["type"], "info.ready");
    ok(socket
        .send(Message::Text(r#"{"id":1,"method":"remote.status"}"#.into()))
        .await);
    let answer = loop {
        let value = frame(&mut socket).await;
        if value.get("id").is_some() {
            break value;
        }
    };
    // A remote core never offers phone access.
    assert_eq!(answer["error"]["code"], "unavailable");
}

#[tokio::test]
async fn without_fake_ssh_there_is_no_bridge() {
    let daemon = start(&[]);
    let refused =
        tokio_tungstenite::connect_async(ws(&daemon, "/ws/ssh/box1", &daemon.token)).await;
    assert!(refused.is_err());
}
