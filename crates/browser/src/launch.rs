//! Finding the user's Chromium and starting it in the session's sandbox
//! with the CDP pipe on fds 3 and 4.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{CommandLine, ProfileNetwork, Result, Sandbox, SandboxProfile};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::process::{Child, Command};

/// Names looked up on `PATH`, in order, when no path is configured.
pub(crate) const NAMES: [&str; 4] = [
    "chromium",
    "chromium-browser",
    "google-chrome",
    "google-chrome-stable",
];

/// The port inside the sandbox where its helper bridges the netproxy.
const PROXY: &str = "http://127.0.0.1:3128";

/// Moves the pipe onto the fds Chromium reads (3) and writes (4), keeps
/// its stdout off it, and becomes the sandboxed line: no `unsafe` needed.
const ON_PIPE: &str = r#"exec "$@" 3<&0 4>&1 </dev/null >&2"#;

/// The configured path if it is a program, else the first of [`NAMES`] on
/// `path`.
pub(crate) fn find(configured: Option<&Path>, path: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    if let Some(configured) = configured {
        return runnable(configured).then(|| configured.to_path_buf());
    }
    let dirs: Vec<PathBuf> = path.map(|p| std::env::split_paths(p).collect())?;
    NAMES.iter().find_map(|name| {
        dirs.iter()
            .map(|dir| dir.join(name))
            .find(|candidate| candidate.is_absolute() && runnable(candidate))
    })
}

fn runnable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        path.metadata()
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

/// Chromium's own arguments for a session's browser. Its own sandbox is
/// off: it needs new user namespaces, which the Ovra sandbox around it
/// forbids, and that sandbox already holds every process of it.
pub(crate) fn args(profile_dir: &Path, network: &ProfileNetwork) -> Vec<OsString> {
    let mut args: Vec<OsString> = [
        "--headless=new",
        "--remote-debugging-pipe",
        "--no-sandbox",
        "--no-first-run",
        "--no-default-browser-check",
        "--disable-extensions",
        "--disable-sync",
        "--disable-background-networking",
        "--disable-component-update",
        "--disable-default-apps",
        "--disable-dev-shm-usage",
        "--password-store=basic",
        "--disable-quic",
        "--force-webrtc-ip-handling-policy=disable_non_proxied_udp",
        "--mute-audio",
    ]
    .iter()
    .map(OsString::from)
    .collect();
    let mut dir = OsString::from("--user-data-dir=");
    dir.push(profile_dir);
    args.push(dir);
    if matches!(network, ProfileNetwork::Proxy { .. }) {
        args.push(format!("--proxy-server={PROXY}").into());
        // Loopback goes through the netproxy too, which knows the servers.
        args.push("--proxy-bypass-list=<-loopback>".into());
    }
    args.push("about:blank".into());
    args
}

/// A started browser: its pipe and, for a real one, its process.
pub(crate) struct Launched {
    pub(crate) reader: Box<dyn AsyncRead + Unpin + Send>,
    pub(crate) writer: Box<dyn AsyncWrite + Unpin + Send>,
    pub(crate) child: Option<Child>,
    /// The last lines it wrote to stderr, once it closed it.
    pub(crate) stderr: Option<tokio::task::JoinHandle<Vec<String>>>,
}

impl std::fmt::Debug for Launched {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Launched")
            .field("child", &self.child)
            .finish_non_exhaustive()
    }
}

/// Starts a session's browser. The seam tests replace with a scripted
/// CDP peer.
pub(crate) trait Launch: Send + Sync + std::fmt::Debug {
    /// Starts it in `profile` with `profile_dir`, already its `work_dir`.
    fn launch(&self, profile: &SandboxProfile, profile_dir: &Path) -> Result<Launched>;
}

/// The user's Chromium through the sandbox.
#[derive(Debug)]
pub(crate) struct Chromium {
    pub(crate) program: PathBuf,
    pub(crate) sandbox: Arc<dyn Sandbox>,
}

impl Chromium {
    /// The line that starts it, through `/bin/sh` for the fds.
    pub(crate) fn line(&self, profile: &SandboxProfile, profile_dir: &Path) -> Result<CommandLine> {
        if matches!(profile.network, ProfileNetwork::Open) {
            return Err(ProtocolError::new(
                ErrorCode::Unavailable,
                "the agent browser needs the session's netproxy",
            ));
        }
        let inner = CommandLine {
            program: self.program.clone(),
            args: args(profile_dir, &profile.network),
            env: Vec::new(),
            cwd: profile_dir.to_path_buf(),
        };
        let wrapped = self.sandbox.wrap(profile, inner)?;
        let mut args: Vec<OsString> = vec!["-c".into(), ON_PIPE.into(), "sh".into()];
        args.push(wrapped.program.into_os_string());
        args.extend(wrapped.args);
        Ok(CommandLine {
            program: "/bin/sh".into(),
            args,
            env: wrapped.env,
            cwd: wrapped.cwd,
        })
    }
}

impl Launch for Chromium {
    fn launch(&self, profile: &SandboxProfile, profile_dir: &Path) -> Result<Launched> {
        let line = self.line(profile, profile_dir)?;
        let mut command = Command::new(&line.program);
        command
            .args(&line.args)
            .current_dir(&line.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        for (name, value) in &line.env {
            match value {
                Some(value) => command.env(name, value),
                None => command.env_remove(name),
            };
        }
        let mut child = command.spawn().map_err(|e| {
            ProtocolError::new(
                ErrorCode::Unavailable,
                format!("the browser did not start: {e}"),
            )
        })?;
        let (Some(writer), Some(reader)) = (child.stdin.take(), child.stdout.take()) else {
            return Err(ProtocolError::new(
                ErrorCode::Internal,
                "no pipe to the browser",
            ));
        };
        let stderr = child
            .stderr
            .take()
            .map(|stderr| tokio::spawn(last_lines(stderr)));
        Ok(Launched {
            reader: Box::new(reader),
            writer: Box::new(writer),
            child: Some(child),
            stderr,
        })
    }
}

/// Reads `stderr` to its end, keeping the last few lines: why a browser
/// that did not start stopped.
async fn last_lines(stderr: tokio::process::ChildStderr) -> Vec<String> {
    use tokio::io::AsyncBufReadExt as _;
    let mut lines = tokio::io::BufReader::new(stderr).lines();
    let mut last = std::collections::VecDeque::new();
    while let Ok(Some(line)) = lines.next_line().await {
        if last.len() == 3 {
            last.pop_front();
        }
        last.push_back(line.chars().take(300).collect());
    }
    last.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ovra_core::fakes::FakeSandbox;

    fn program(dir: &Path, name: &str, mode: u32) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, "#!/bin/sh\n").expect("write");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).expect("chmod");
        }
        path
    }

    #[test]
    fn finds_the_first_known_name_on_path_and_never_a_plain_file() {
        let root = std::env::temp_dir().join(format!("ovra-browser-find-{}", std::process::id()));
        let (a, b) = (root.join("a"), root.join("b"));
        std::fs::create_dir_all(&a).expect("a");
        std::fs::create_dir_all(&b).expect("b");
        program(&a, "chromium", 0o644);
        let chrome = program(&a, "google-chrome", 0o755);
        let browser = program(&b, "chromium-browser", 0o755);
        let path = std::env::join_paths([&a, &b]).expect("path");
        assert_eq!(find(None, Some(&path)), Some(browser.clone()));
        assert_eq!(find(Some(&chrome), Some(&path)), Some(chrome));
        assert_eq!(find(Some(&a.join("missing")), Some(&path)), None);
        assert_eq!(find(None, Some(std::ffi::OsStr::new(""))), None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_line_runs_the_sandboxed_chromium_on_the_pipe_behind_the_proxy() {
        let chromium = Chromium {
            program: "/usr/bin/chromium".into(),
            sandbox: Arc::new(FakeSandbox::available()),
        };
        let mut profile = SandboxProfile {
            kind: ovra_core::services::ProfileKind::Browser,
            work_dir: "/w".into(),
            read: Vec::new(),
            write: vec!["/t/profile".into()],
            expose: Vec::new(),
            forward: Vec::new(),
            network: ProfileNetwork::Proxy {
                address: "/t/proxy.sock".into(),
                ca_bundle: None,
            },
        };
        let line = chromium
            .line(&profile, Path::new("/t/profile"))
            .expect("line");
        insta::assert_debug_snapshot!(line);
        profile.network = ProfileNetwork::Open;
        let refused = chromium.line(&profile, Path::new("/t/profile")).err();
        assert_eq!(refused.map(|e| e.code), Some(ErrorCode::Unavailable));
    }
}
