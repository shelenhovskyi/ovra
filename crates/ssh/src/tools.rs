//! The person's own OpenSSH tools: found once on `PATH`, checked, and run
//! with a short environment and a time limit.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use ovra_core::protocol::{ErrorCode, ProtocolError};

/// OpenSSH older than this lacks options the command line needs
/// (`SessionType`, `StdinNull`, `ForkAfterAuthentication`).
const OLDEST: (u32, u32) = (8, 7);
/// How long `ssh -G` or `ssh-keygen` may take: the person's config can
/// run commands (`Match exec`) or look names up.
const LIMIT: Duration = Duration::from_secs(10);

/// The variables `ssh` keeps: enough to find the person's keys, agent and
/// config, nothing that could be sent with `SendEnv`.
const KEPT: [&str; 8] = [
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "LANG",
    "SSH_AUTH_SOCK",
    "XDG_RUNTIME_DIR",
    "SYSTEMROOT",
];

#[derive(Debug, Clone)]
pub(crate) struct Tools {
    pub(crate) ssh: PathBuf,
    pub(crate) keygen: PathBuf,
}

impl Tools {
    /// `ssh` and `ssh-keygen` from `path` (a `PATH` value), skipping empty
    /// and relative entries, with `ssh` checked to be new enough.
    pub(crate) fn find(path: Option<OsString>) -> Result<Self, ProtocolError> {
        let dirs: Vec<PathBuf> = path
            .map(|p| {
                std::env::split_paths(&p)
                    .filter(|d| d.is_absolute())
                    .collect()
            })
            .unwrap_or_default();
        let find = |name: &str| {
            let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
            dirs.iter().map(|d| d.join(&file)).find(|p| p.is_file())
        };
        let (Some(ssh), Some(keygen)) = (find("ssh"), find("ssh-keygen")) else {
            return Err(no_ssh(
                "Ovra could not find OpenSSH (ssh and ssh-keygen) on PATH",
            ));
        };
        let tools = Self { ssh, keygen };
        let out = tools.run(Command::new(&tools.ssh).arg("-V"), None)?;
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

    /// Runs `command` with the kept variables only, `input` on stdin, and
    /// gives up after [`LIMIT`].
    pub(crate) fn run(
        &self,
        command: &mut Command,
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
        command
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .map_err(|e| no_ssh(&format!("could not run OpenSSH: {e}")))?;
        if let (Some(text), Some(mut stdin)) = (input, child.stdin.take()) {
            use std::io::Write;
            let _ = stdin.write_all(text.as_bytes());
        }
        // Read while waiting, so that a long known_hosts cannot fill the pipe.
        let read = |pipe: Option<Box<dyn std::io::Read + Send>>| {
            std::thread::spawn(move || {
                let mut bytes = Vec::new();
                if let Some(mut pipe) = pipe {
                    let _ = pipe.read_to_end(&mut bytes);
                }
                bytes
            })
        };
        let stdout = read(child.stdout.take().map(|p| Box::new(p) as Box<_>));
        let stderr = read(child.stderr.take().map(|p| Box::new(p) as Box<_>));
        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if started.elapsed() < LIMIT => {
                    std::thread::sleep(Duration::from_millis(2));
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(no_ssh("OpenSSH took too long reading your SSH config"));
                }
            }
        };
        Ok(Output {
            status,
            stdout: stdout.join().unwrap_or_default(),
            stderr: stderr.join().unwrap_or_default(),
        })
    }

    pub(crate) fn ssh(&self) -> Command {
        Command::new(&self.ssh)
    }
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
