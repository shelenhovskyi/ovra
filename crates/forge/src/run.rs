//! Starting `gh` and `git`: found on the person's `PATH` (as the runtime
//! finds `claude`), run without a shell with the person's environment plus
//! the variables that keep them from waiting for input, under a timeout,
//! and killed when the call is dropped. On Unix each runs in its own
//! process group, which is killed with it, so a helper git started (`ssh`,
//! a credential helper) ends too; on Windows only `gh` or `git` itself is
//! killed.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::Result;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};

use crate::parse::scrub;

/// `git fetch` and `git push`, which may wait on the network.
pub(crate) const GIT_TIMEOUT: Duration = Duration::from_secs(120);
/// Every `gh` call, and the local `git` questions.
pub(crate) const GH_TIMEOUT: Duration = Duration::from_secs(30);

/// Added to the person's environment, which is otherwise passed unchanged:
/// `gh` and git's credential helper find the login themselves.
const QUIET: [(&str, &str); 4] = [
    ("GH_PROMPT_DISABLED", "1"),
    ("GH_NO_UPDATE_NOTIFIER", "1"),
    ("NO_COLOR", "1"),
    ("GIT_TERMINAL_PROMPT", "0"),
];

/// The programs to run; `None` when not installed.
#[derive(Debug, Clone, Default)]
pub(crate) struct Programs {
    pub(crate) gh: Option<PathBuf>,
    pub(crate) git: Option<PathBuf>,
}

impl Programs {
    /// The ones on the login shell's `PATH` (else Ovra's own), then where
    /// installers put them (a desktop app often starts without the
    /// shell's `PATH`). Looked up per call, so installing `gh` while Ovra
    /// runs is enough.
    pub(crate) fn find() -> Self {
        let path = search_path();
        Self {
            gh: find_in("gh", path.as_ref()),
            git: find_in("git", path.as_ref()),
        }
    }
}

/// The login shell's `PATH`, else Ovra's own.
fn search_path() -> Option<OsString> {
    ovra_core::child_path::or_inherited(std::env::var_os("PATH"))
}

/// The absolute directories of `path`, in order.
fn absolute_dirs(path: Option<&OsString>) -> Vec<PathBuf> {
    path.map(|p| {
        std::env::split_paths(p)
            .filter(|d| d.is_absolute())
            .collect()
    })
    .unwrap_or_default()
}

/// `name` in an absolute directory of `path` or of the installers' usual
/// ones. On Windows only `<name>.exe`: a `.cmd` would run through `cmd`.
fn find_in(name: &str, path: Option<&OsString>) -> Option<PathBuf> {
    let mut dirs = absolute_dirs(path);
    let file = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        dirs.extend(["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"].map(PathBuf::from));
        name.to_owned()
    };
    dirs.into_iter()
        .map(|dir| dir.join(&file))
        .find(|candidate| candidate.is_file())
}

/// What a finished process wrote.
#[derive(Debug)]
pub(crate) struct Output {
    pub(crate) success: bool,
    pub(crate) code: Option<i32>,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

/// A started child that, unless it was waited for, is killed with its
/// process group when dropped (a timeout or a dropped call). It is not
/// reaped before that, so the group id cannot belong to anything else.
struct Group(Child);

impl Drop for Group {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(pid) = self.0.id().and_then(|id| i32::try_from(id).ok()) {
            use rustix::process::{Pid, Signal, kill_process_group};
            if let Some(group) = Pid::from_raw(pid) {
                // Gone already when this fails.
                let _ = kill_process_group(group, Signal::KILL);
            }
        }
        // Then `kill_on_drop` ends the child itself, on every system.
    }
}

async fn read_all(pipe: Option<impl AsyncRead + Unpin>) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    if let Some(mut pipe) = pipe {
        pipe.read_to_end(&mut bytes).await?;
    }
    Ok(bytes)
}

/// Runs `program args` in `dir` with `stdin` (or none) and waits at most
/// `limit`. `what` names it in errors, such as `git push`.
pub(crate) async fn run(
    program: &Path,
    dir: &Path,
    what: &str,
    args: &[&str],
    stdin: Option<&str>,
    limit: Duration,
) -> Result<Output> {
    let mut command = Command::new(program);
    #[cfg(unix)]
    command.process_group(0);
    command
        .env(
            "PATH",
            ovra_core::child_path::absolute(std::env::var_os("PATH")),
        )
        .args(args)
        .envs(QUIET)
        .current_dir(dir)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let work = async {
        let mut child = Group(command.spawn()?);
        let pipe = child.0.stdin.take();
        let feed = async move {
            if let (Some(mut pipe), Some(text)) = (pipe, stdin) {
                // A process that exits without reading closes the pipe;
                // its exit status says what happened.
                let _ = pipe.write_all(text.as_bytes()).await;
            }
        };
        let (stdout, stderr) = (child.0.stdout.take(), child.0.stderr.take());
        let ((), stdout, stderr, status) =
            tokio::join!(feed, read_all(stdout), read_all(stderr), child.0.wait());
        std::io::Result::Ok((status?, stdout?, stderr?))
    };
    let output = tokio::time::timeout(limit, work)
        .await
        .map_err(|_| {
            ProtocolError::new(
                ErrorCode::Unavailable,
                format!("{what} did not finish within {} s", limit.as_secs()),
            )
        })?
        .map_err(|e| {
            ProtocolError::new(ErrorCode::Internal, format!("could not run {what}: {e}"))
        })?;
    let (status, stdout, stderr) = output;
    Ok(Output {
        success: status.success(),
        code: status.code(),
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    })
}

/// An error for a process that failed: the end of what it wrote to stderr,
/// without the user part of any URL in it. Scrubbed before it is cut: a
/// cut inside a URL would leave `token@host` without its `://`.
pub(crate) fn failed(what: &str, output: &Output) -> ProtocolError {
    let scrubbed = scrub(output.stderr.trim());
    let text = scrubbed.as_str();
    let mut start = text.len().saturating_sub(1000);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    ProtocolError::new(
        ErrorCode::Internal,
        format!("{what} failed: {}", &text[start..]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn programs_are_found_only_in_absolute_directories() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let file = if cfg!(windows) { "gh.exe" } else { "gh" };
        std::fs::write(dir.path().join(file), "").unwrap_or_else(|e| panic!("{e}"));
        let path = std::env::join_paths([Path::new("relative"), dir.path()])
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(find_in("gh", Some(&path)), Some(dir.path().join(file)));
        let relative = OsString::from(".");
        assert_ne!(
            find_in("Cargo.toml", Some(&relative)),
            Some(PathBuf::from("./Cargo.toml"))
        );
    }

    #[test]
    fn an_error_is_scrubbed_before_it_is_cut() {
        // The cut falls inside the URL's user part, after its `://`.
        let stderr = format!(
            "{}https://user:secret@h.example {}",
            "a".repeat(100),
            "b".repeat(980)
        );
        let output = Output {
            success: false,
            code: Some(128),
            stdout: String::new(),
            stderr,
        };
        let message = failed("git push", &output).message;
        assert!(!message.contains("cret"), "{message}");
        assert!(message.ends_with(&"b".repeat(980)), "{message}");
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn a_dropped_call_kills_what_the_program_started() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let (script, pid) = (dir.path().join("git"), dir.path().join("pid"));
        let text = "#!/bin/sh\nsleep 600 &\necho $! > \"$1.tmp\"\nmv \"$1.tmp\" \"$1\"\n\
                    exec sleep 600\n";
        std::fs::write(&script, text).unwrap_or_else(|e| panic!("{e}"));
        let mode = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(&script, mode).unwrap_or_else(|e| panic!("{e}"));
        let arg = pid.to_string_lossy().into_owned();
        let args = [arg.as_str()];
        let started = run(&script, dir.path(), "git", &args, None, GIT_TIMEOUT);
        let written = async {
            while !pid.exists() {
                tokio::task::yield_now().await;
            }
        };
        tokio::select! {
            ran = started => panic!("finished: {ran:?}"),
            () = written => {}
        }
        let pid = std::fs::read_to_string(&pid).unwrap_or_default();
        let stat = format!("/proc/{}/stat", pid.trim());
        // Dead (gone, or a zombie nobody reaped yet) soon after the drop.
        let begun = std::time::Instant::now();
        loop {
            let state = std::fs::read_to_string(&stat).unwrap_or_default();
            let state = state.rsplit(") ").next().and_then(|s| s.chars().next());
            if matches!(state, None | Some('Z' | 'X')) {
                break;
            }
            assert!(begun.elapsed().as_secs() < 2, "{stat} still runs");
            std::thread::yield_now();
        }
    }
}
