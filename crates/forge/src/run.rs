//! Starting `gh` and `git`: found on the person's `PATH` (as the runtime
//! finds `claude`), run without a shell with the person's environment plus
//! the variables that keep them from waiting for input, under a timeout,
//! and killed when the call is dropped.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::Result;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

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
        let path = ovra_core::child_path::or_inherited(std::env::var_os("PATH"));
        Self {
            gh: find_in("gh", path.as_ref()),
            git: find_in("git", path.as_ref()),
        }
    }
}

/// `name` in an absolute directory of `path` or of the installers' usual
/// ones. On Windows only `<name>.exe`: a `.cmd` would run through `cmd`.
fn find_in(name: &str, path: Option<&OsString>) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = path
        .map(|p| std::env::split_paths(p).collect())
        .unwrap_or_default();
    let file = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        dirs.extend(["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"].map(PathBuf::from));
        name.to_owned()
    };
    dirs.into_iter()
        .filter(|dir| dir.is_absolute())
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
    if let Some(path) = ovra_core::child_path::login() {
        command.env("PATH", path);
    }
    command
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
        let mut child = command.spawn()?;
        let pipe = child.stdin.take();
        let feed = async move {
            if let (Some(mut pipe), Some(text)) = (pipe, stdin) {
                // A process that exits without reading closes the pipe;
                // its exit status says what happened.
                let _ = pipe.write_all(text.as_bytes()).await;
            }
        };
        let ((), output) = tokio::join!(feed, child.wait_with_output());
        output
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
    Ok(Output {
        success: output.status.success(),
        code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

/// An error for a process that failed: the end of what it wrote to stderr,
/// without the user part of any URL in it.
pub(crate) fn failed(what: &str, output: &Output) -> ProtocolError {
    let text = output.stderr.trim();
    let mut start = text.len().saturating_sub(1000);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    ProtocolError::new(
        ErrorCode::Internal,
        format!("{what} failed: {}", scrub(&text[start..])),
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
}
