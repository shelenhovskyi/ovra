//! `ovra-daemon --stdio` as the laptop's `ssh` runs it (ADR 0016).

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

#[test]
fn a_remote_core_says_hello_answers_and_stops_when_its_client_goes() {
    let data = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let mut child = Command::new(env!("CARGO_BIN_EXE_ovra-daemon"))
        .arg("--stdio")
        .arg("--data-dir")
        .arg(data.path())
        .env("RUST_LOG", "warn")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|e| panic!("{e}"));
    let mut input = child.stdin.take().unwrap_or_else(|| panic!("no stdin"));
    let mut output = BufReader::new(child.stdout.take().unwrap_or_else(|| panic!("no stdout")));
    let mut line = || {
        let mut text = String::new();
        output
            .read_line(&mut text)
            .unwrap_or_else(|e| panic!("{e}"));
        serde_json::from_str::<serde_json::Value>(&text).unwrap_or_else(|e| panic!("{e}: {text}"))
    };

    let hello = line();
    assert_eq!(hello["hello"]["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(hello["hello"]["protocol"], 1);
    assert_eq!(hello["hello"]["os"], std::env::consts::OS);
    assert_eq!(line()["event"]["type"], "info.ready");

    writeln!(input, r#"{{"id":1,"method":"projects.list"}}"#).unwrap_or_else(|e| panic!("{e}"));
    let answer = line();
    assert_eq!(answer["id"], 1);
    assert_eq!(answer["result"]["type"], "projects.list");
    // A remote machine never offers phone access.
    writeln!(input, r#"{{"id":2,"method":"remote.status"}}"#).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(line()["error"]["code"], "unavailable");

    drop(input);
    let status = child.wait().unwrap_or_else(|e| panic!("{e}"));
    assert!(status.success(), "{status}");
}
