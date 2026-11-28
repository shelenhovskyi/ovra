//! `--attach` and `--core` as `ssh` runs them (ssh-remotes.md, section 6):
//! the core outlives an attach, takes the next one back with a resync,
//! and ends on `{"quit":{}}`.
#![cfg(target_os = "linux")]

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

fn ok<T, E: std::fmt::Display>(r: Result<T, E>) -> T {
    r.unwrap_or_else(|e| panic!("{e}"))
}

struct Attached {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl Attached {
    fn start(data: &Path, home: &Path, extra: &[&str]) -> Self {
        let mut child = ok(Command::new(env!("CARGO_BIN_EXE_ovra-daemon"))
            .arg("--attach")
            .args(extra)
            .arg("--data-dir")
            .arg(data)
            .env("HOME", home)
            .env("RUST_LOG", "error")
            .env("SSH_CONNECTION", "10.0.0.1 1 10.0.0.2 22")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn());
        let input = child.stdin.take().unwrap_or_else(|| panic!("no stdin"));
        let output = BufReader::new(child.stdout.take().unwrap_or_else(|| panic!("no stdout")));
        Self {
            child,
            input,
            output,
        }
    }

    fn line(&mut self) -> String {
        let mut line = String::new();
        ok(self.output.read_line(&mut line));
        line.trim_end().to_owned()
    }

    fn send(&mut self, line: &str) {
        ok(writeln!(self.input, "{line}"));
        ok(self.input.flush());
    }
}

impl Drop for Attached {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The core's pid, from `/proc`: the process whose command line has
/// `--core` and this data folder.
fn core_pid(data: &Path) -> Option<String> {
    let data = data.to_string_lossy().into_owned();
    std::fs::read_dir("/proc")
        .ok()?
        .flatten()
        .find_map(|entry| {
            let cmdline = std::fs::read(entry.path().join("cmdline")).ok()?;
            let args: Vec<String> = cmdline
                .split(|&b| b == 0)
                .map(|a| String::from_utf8_lossy(a).into_owned())
                .collect();
            (args.iter().any(|a| a == "--core") && args.contains(&data))
                .then(|| entry.file_name().to_string_lossy().into_owned())
        })
}

#[test]
fn the_core_outlives_an_attach_and_ends_on_quit() {
    let dir = ok(tempfile::tempdir());
    let data = dir.path().join("data");
    let first_hello = {
        let mut first = Attached::start(&data, dir.path(), &[]);
        let hello = first.line();
        assert!(hello.starts_with(r#"{"hello":"#), "{hello}");
        assert!(first.line().contains("info.ready"));
        first.send(r#"{"ping":{}}"#);
        hello
        // Dropped: the SSH session ends.
    };
    let pid = core_pid(&data).unwrap_or_else(|| panic!("no core running"));
    // The SSH session's variables stayed out of the core.
    let environ = ok(std::fs::read(format!("/proc/{pid}/environ")));
    assert!(!String::from_utf8_lossy(&environ).contains("SSH_CONNECTION"));

    let mut second = Attached::start(&data, dir.path(), &[]);
    // The same core: the same start time.
    assert_eq!(second.line(), first_hello);
    assert!(second.line().contains("info.ready"));
    assert!(second.line().contains("info.resync"));
    second.send(r#"{"quit":{}}"#);
    let mut waited = 0;
    while core_pid(&data).is_some() && waited < 200 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        waited += 1;
    }
    assert!(core_pid(&data).is_none(), "the core still runs");
}
