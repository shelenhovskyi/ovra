//! The person's own OpenSSH tools: found once on `PATH`, checked, and run
//! with a short environment and a time limit.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};
use std::time::Duration;

use ovra_core::protocol::{ErrorCode, ProtocolError};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};

/// OpenSSH older than this lacks options the command line needs
/// (`SessionType`, `StdinNull`, `ForkAfterAuthentication`).
const OLDEST: (u32, u32) = (8, 7);
/// How long `ssh -G` or `ssh-keygen` may take: the person's config can
/// run commands (`Match exec`) or look names up.
const LIMIT: Duration = Duration::from_secs(10);
/// The most either pipe may carry; more fails the call.
const OUTPUT_MAX: u64 = 4 * 1024 * 1024;

/// The variables `ssh` keeps: enough to find the person's keys, agent and
/// config, nothing that could be sent with `SendEnv`. The last four are
/// how Windows and its OpenSSH find the system and `~/.ssh`.
const KEPT: [&str; 11] = [
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "LANG",
    "SSH_AUTH_SOCK",
    "XDG_RUNTIME_DIR",
    "SYSTEMROOT",
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
];

#[derive(Debug, Clone)]
pub(crate) struct Tools {
    pub(crate) ssh: PathBuf,
    pub(crate) keygen: PathBuf,
    /// [`LIMIT`], shorter in tests.
    pub(crate) limit: Duration,
}

impl Tools {
    /// `ssh` and `ssh-keygen` from `path` (a `PATH` value), skipping empty
    /// and relative entries and files that cannot run, with `ssh` checked
    /// to be new enough.
    pub(crate) async fn find(path: Option<OsString>) -> Result<Self, ProtocolError> {
        let dirs: Vec<PathBuf> = path
            .map(|p| {
                std::env::split_paths(&p)
                    .filter(|d| d.is_absolute())
                    .collect()
            })
            .unwrap_or_default();
        let find = |name: &str| {
            let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
            dirs.iter().map(|d| d.join(&file)).find(|p| runnable(p))
        };
        let (Some(ssh), Some(keygen)) = (find("ssh"), find("ssh-keygen")) else {
            return Err(no_ssh(
                "Ovra could not find OpenSSH (ssh and ssh-keygen) on PATH",
            ));
        };
        let tools = Self {
            ssh,
            keygen,
            limit: LIMIT,
        };
        let mut command = tools.ssh();
        command.arg("-V");
        let out = tools.run(command, None).await?;
        // `ssh -V` prints to stderr.
        let text = String::from_utf8_lossy(&out.stderr);
        match version(&text) {
            Some(found) if found >= OLDEST => Ok(tools),
            _ => Err(no_ssh(&format!(
                "Ovra needs OpenSSH {}.{} or later; {} says {}",
                OLDEST.0,
                OLDEST.1,
                tools.ssh.display(),
                text.trim()
            ))),
        }
    }

    /// Runs `command` with the kept variables only and `input` on stdin, in
    /// its own process group. Gives up after `limit`, and then ends the
    /// whole group: a command the person's config ran in the background
    /// (`Match exec "... &"`) can hold the pipes open after `ssh` exits.
    pub(crate) async fn run(
        &self,
        mut command: Command,
        input: Option<&str>,
    ) -> Result<Output, ProtocolError> {
        command.env_clear();
        for (name, value) in std::env::vars_os() {
            let keep = name
                .to_str()
                .is_some_and(|n| KEPT.contains(&n) || n.starts_with("LC_"));
            if keep {
                command.env(name, value);
            }
        }
        #[cfg(unix)]
        command.process_group(0);
        command
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = Group(
            command
                .spawn()
                .map_err(|e| no_ssh(&format!("could not run OpenSSH: {e}")))?,
        );
        let pipe = child.0.stdin.take();
        let feed = async move {
            if let (Some(mut pipe), Some(text)) = (pipe, input) {
                // A process that exits without reading closes the pipe;
                // its exit status says what happened.
                let _ = pipe.write_all(text.as_bytes()).await;
            }
        };
        let (stdout, stderr) = (child.0.stdout.take(), child.0.stderr.take());
        let work = async {
            // The pipes first, then the exit: until it is reaped, the
            // child's id names its group and nothing else.
            let ((), stdout, stderr) = tokio::join!(feed, read_all(stdout), read_all(stderr));
            let (stdout, stderr) = (stdout?, stderr?);
            let status = child.0.wait().await?;
            std::io::Result::Ok(Output {
                status,
                stdout,
                stderr,
            })
        };
        tokio::time::timeout(self.limit, work)
            .await
            .map_err(|_| no_ssh("OpenSSH took too long reading your SSH config"))?
            .map_err(|e| no_ssh(&format!("could not run OpenSSH: {e}")))
    }

    pub(crate) fn ssh(&self) -> Command {
        Command::new(&self.ssh)
    }

    pub(crate) fn keygen(&self) -> Command {
        Command::new(&self.keygen)
    }
}

/// A file this user can run: on Unix, one with an execute bit.
fn runnable(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.is_file() && meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    meta.is_file()
}

/// A child that, unless it was reaped, is killed with its process group
/// when dropped (a timeout, too much output or a dropped call).
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

/// All of `pipe`, or an error past [`OUTPUT_MAX`].
async fn read_all(pipe: Option<impl AsyncRead + Unpin>) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    if let Some(pipe) = pipe {
        pipe.take(OUTPUT_MAX + 1).read_to_end(&mut bytes).await?;
    }
    if bytes.len() as u64 > OUTPUT_MAX {
        return Err(std::io::Error::other("OpenSSH wrote too much"));
    }
    Ok(bytes)
}

/// `(major, minor)` from `OpenSSH_9.6p1 ...`, or `OpenSSH_for_Windows_9.5p1`.
fn version(text: &str) -> Option<(u32, u32)> {
    let rest = text.split("OpenSSH_").nth(1)?;
    let rest = rest.strip_prefix("for_Windows_").unwrap_or(rest);
    let (major, rest) = rest.split_once('.')?;
    let minor: String = rest.chars().take_while(char::is_ascii_digit).collect();
    Some((major.parse().ok()?, minor.parse().ok()?))
}

pub(crate) fn no_ssh(message: &str) -> ProtocolError {
    ProtocolError::new(ErrorCode::Unavailable, message)
}

#[cfg(test)]
mod tests {
    use super::version;

    #[test]
    fn versions_are_read_from_ssh_v() {
        assert_eq!(
            version("OpenSSH_9.6p1 Ubuntu-3ubuntu13.20, OpenSSL 3.0.13"),
            Some((9, 6))
        );
        assert_eq!(
            version("OpenSSH_for_Windows_9.5p1, LibreSSL 3.8.2"),
            Some((9, 5))
        );
        assert_eq!(version("OpenSSH_8.10p1"), Some((8, 10)));
        assert_eq!(version("Sun_SSH_1.1"), None);
        assert!(version("OpenSSH_8.6p1").is_some_and(|v| v < super::OLDEST));
    }
}
