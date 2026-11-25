//! The login shell's `PATH`, read once when the app starts. A desktop app
//! started from the Finder or a launcher does not get the `PATH` the
//! user's shell sets up, so Homebrew's tools (`git-lfs`, ...) are missing
//! for agents, git and terminals. Like other editors, Ovra asks the user's
//! `$SHELL` for it, as a login and interactive shell, with a time limit.
//! Only `PATH` is read; nothing else from the shell's environment.

use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

/// How long the shell may take. The app's start waits for it, once:
/// usually well under a second, longer with heavy rc files.
const TIMEOUT: Duration = Duration::from_secs(3);

/// Marks `PATH` in the shell's output, which rc files may add to.
const MARK: &str = "_OVRA_LOGIN_PATH_";

/// Reads the login shell's `PATH` and sets it for every program Ovra
/// starts (`ovra_core::child_path`). Call once in the app's `main`, after
/// the sandbox helper and pod checks. Does nothing when the shell fails or
/// takes too long.
pub fn use_login_path() {
    let Some(shell) = std::env::var_os("SHELL").map(PathBuf::from) else {
        return;
    };
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    if let Some(path) = login_path(&shell, &inherited, TIMEOUT) {
        ovra_core::child_path::set_login(path);
    }
}

/// `shell`'s `PATH` merged with `inherited`, or `None` if `shell` is not
/// absolute, fails, prints no `PATH` or takes longer than `timeout`.
fn login_path(shell: &Path, inherited: &OsStr, timeout: Duration) -> Option<OsString> {
    if !shell.is_absolute() {
        return None;
    }
    // `"$PATH"` is the colon-separated list in sh, bash, zsh and fish 3.
    let script = format!("printf '%s' '{MARK}'\"$PATH\"'{MARK}'");
    let mut child = Command::new(shell)
        .args(["-i", "-l", "-c", &script])
        // Its own process group: never the terminal's foreground job, and
        // whatever the rc files start can be stopped with it.
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let (send, receive) = mpsc::channel();
    // Stops at the second mark: a process the rc files leave behind may
    // keep the pipe open.
    std::thread::spawn(move || {
        let mut out = Vec::new();
        let mut chunk = [0; 4096];
        while let Ok(read @ 1..) = stdout.read(&mut chunk) {
            out.extend_from_slice(&chunk[..read]);
            if let Some(path) = parse(&out) {
                let _ = send.send(path);
                return;
            }
        }
    });
    let path = receive.recv_timeout(timeout);
    stop_group(child.id());
    let _ = child.kill();
    let _ = child.wait();
    merge(&path.ok()?, inherited)
}

/// Kills the process group `id` leads, with `kill` (Ovra has no unsafe
/// code to call it directly).
fn stop_group(id: u32) {
    let _ = Command::new("kill")
        .args(["-KILL", "--", &format!("-{id}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// The text between the two marks.
fn parse(out: &[u8]) -> Option<OsString> {
    let out = std::str::from_utf8(out).ok()?;
    let (_, rest) = out.split_once(MARK)?;
    let (path, _) = rest.split_once(MARK)?;
    Some(path.into())
}

/// The absolute entries of `login`, then those of `inherited` it lacks,
/// each once. Relative entries (`.`, empty) would make programs depend on
/// the folder they start in. `None` if `login` has no absolute entry.
fn merge(login: &OsStr, inherited: &OsStr) -> Option<OsString> {
    let mut entries: Vec<PathBuf> = Vec::new();
    for entry in std::env::split_paths(login) {
        if entry.is_absolute() && !entries.contains(&entry) {
            entries.push(entry);
        }
    }
    if entries.is_empty() {
        return None;
    }
    for entry in std::env::split_paths(inherited) {
        if entry.is_absolute() && !entries.contains(&entry) {
            entries.push(entry);
        }
    }
    std::env::join_paths(entries).ok()
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    /// A stand-in for the user's shell: a script that prints `body`.
    fn shell(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("shell");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        path
    }

    #[test]
    fn reads_path_between_the_marks_and_keeps_inherited_entries_after_it() {
        let dir = tempfile::tempdir().expect("temp dir");
        let shell = shell(
            dir.path(),
            &format!(
                "echo 'Welcome!'\nprintf '%s' '{MARK}/opt/homebrew/bin:.::/usr/bin{MARK}'\necho bye"
            ),
        );
        let path = login_path(&shell, OsStr::new("/usr/bin:/bin:relative"), TIMEOUT);
        assert_eq!(path, Some("/opt/homebrew/bin:/usr/bin:/bin".into()));
    }

    #[test]
    fn the_real_command_line_prints_the_shells_path() {
        let dir = tempfile::tempdir().expect("temp dir");
        // Runs the script it is given, like a shell would.
        let shell = shell(
            dir.path(),
            "shift 3; PATH=/opt/tools/bin:/usr/bin; eval \"$1\"",
        );
        let path = login_path(&shell, OsStr::new("/bin"), TIMEOUT);
        assert_eq!(path, Some("/opt/tools/bin:/usr/bin:/bin".into()));
    }

    #[test]
    fn a_process_the_rc_files_leave_behind_neither_delays_nor_outlives_it() {
        let dir = tempfile::tempdir().expect("temp dir");
        let pid = dir.path().join("pid");
        let shell = shell(
            dir.path(),
            &format!(
                "sleep 10 &\necho $! > '{}'\nprintf '%s' '{MARK}/usr/bin{MARK}'\nwait",
                pid.display()
            ),
        );
        let path = login_path(&shell, OsStr::new("/bin"), TIMEOUT);
        assert_eq!(path, Some("/usr/bin:/bin".into()));
        let pid = std::fs::read_to_string(&pid).expect("pid");
        // Gone, or a zombie nobody has reaped yet.
        let ps = Command::new("ps")
            .args(["-o", "stat=", "-p", pid.trim()])
            .output()
            .expect("ps");
        let stat = String::from_utf8_lossy(&ps.stdout);
        assert!(
            stat.trim().is_empty() || stat.trim().starts_with('Z'),
            "still runs: {stat}"
        );
    }

    #[test]
    fn a_shell_that_hangs_fails_or_prints_nothing_changes_nothing() {
        let dir = tempfile::tempdir().expect("temp dir");
        let hangs = shell(dir.path(), "exec sleep 10");
        let short = Duration::from_millis(50);
        assert_eq!(login_path(&hangs, OsStr::new("/bin"), short), None);
        let fails = shell(dir.path(), "exit 1");
        assert_eq!(login_path(&fails, OsStr::new("/bin"), TIMEOUT), None);
        assert_eq!(
            login_path(Path::new("zsh"), OsStr::new("/bin"), TIMEOUT),
            None
        );
        assert_eq!(merge(OsStr::new(".:"), OsStr::new("/bin")), None);
    }
}
