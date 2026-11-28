//! `--attach` and `--core` as `ssh` runs them (ssh-remotes.md, section 6):
//! the core outlives an attach, takes the next one back with a resync,
//! ends on `{"quit":{}}`, gives way to a restart, and says `busy` while a
//! `--stdio` holds the data folder.
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

/// Kills the core of a data folder when a test ends, passed or not.
struct Core<'a>(&'a Path);

impl Drop for Core<'_> {
    fn drop(&mut self) {
        if let Some(pid) = core_pid(self.0) {
            let _ = Command::new("kill").arg("-9").arg(pid).status();
        }
    }
}

/// Waits until `data` has no core, for at most two seconds.
fn gone(data: &Path) -> bool {
    for _ in 0..200 {
        if core_pid(data).is_none() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    false
}

#[test]
fn the_core_outlives_an_attach_and_ends_on_quit() {
    let dir = ok(tempfile::tempdir());
    let data = dir.path().join("data");
    let _core = Core(&data);
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
    assert!(gone(&data), "the core still runs");
}

#[test]
fn a_restart_ends_the_running_core_and_starts_a_new_one() {
    let dir = ok(tempfile::tempdir());
    let data = dir.path().join("data");
    let _core = Core(&data);
    let old_hello = {
        let mut first = Attached::start(&data, dir.path(), &[]);
        first.line()
    };
    let old = core_pid(&data).unwrap_or_else(|| panic!("no core running"));
    let mut next = Attached::start(&data, dir.path(), &["--restart"]);
    let hello = next.line();
    assert!(hello.starts_with(r#"{"hello":"#), "{hello}");
    assert_ne!(hello, old_hello);
    assert_ne!(core_pid(&data), Some(old));
    next.send(r#"{"quit":{}}"#);
    assert!(gone(&data));
}

#[test]
fn attaching_while_a_stdio_holds_the_folder_says_busy() {
    let dir = ok(tempfile::tempdir());
    let data = dir.path().join("data");
    let _core = Core(&data);
    let mut stdio = ok(Command::new(env!("CARGO_BIN_EXE_ovra-daemon"))
        .arg("--stdio")
        .arg("--data-dir")
        .arg(&data)
        .env("HOME", dir.path())
        .env("RUST_LOG", "error")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn());
    let mut out = BufReader::new(stdio.stdout.take().unwrap_or_else(|| panic!("no stdout")));
    let mut hello = String::new();
    ok(out.read_line(&mut hello));
    assert!(hello.starts_with(r#"{"hello":"#), "{hello}");
    let mut attach = Attached::start(&data, dir.path(), &[]);
    assert_eq!(attach.line(), r#"{"busy":{}}"#);
    let _ = stdio.kill();
    let _ = stdio.wait();
}
