//! Installing Ovra on a remote machine (`docs/specs/ssh-remotes.md`,
//! section 7): `uname -sm` to learn its architecture, then this laptop's own
//! build of `ovra-daemon` on stdin to one fixed `sh -c` command. Both are
//! fixed strings; the only thing put into them is the build's SHA-256,
//! fixed when the app was built. The laptop never runs what it uploads.

use std::process::Stdio;
use std::time::Duration;

use ovra_core::protocol::ssh::RemoteHost;
use ovra_core::ssh::{DaemonPath, HostId};
use sha2::{Digest, Sha256 as Hasher};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

use crate::link::{self, Failure, Reason};
use crate::{OpenSsh, What, tools};

/// The largest build Ovra uploads.
pub const UPLOAD_MAX: usize = 128 << 20;
/// How long the upload may take, all of it.
const UPLOAD_WAIT: Duration = Duration::from_secs(5 * 60);
/// How long `uname` may take, connecting included.
const UNAME_WAIT: Duration = Duration::from_secs(60);
/// What is kept of the machine's answers.
const SAID_MAX: usize = 16 * 1024;
/// The upload goes in pieces of this size, each one reported.
const PIECE: usize = 256 * 1024;

/// The first command: which build the machine needs.
pub(crate) const UNAME: &str = "uname -sm";

/// Why an upload that ended without a word failed.
const REFUSED: &str = "the remote machine refused the upload: its folder ~/.local/share/ovra \
     may be a link or someone else's, or the upload was cut short";

/// Where the install puts Ovra, as the machine's record names it then.
pub const INSTALLED: &str = "~/.local/share/ovra/bin/ovra-daemon";

/// The Linux builds Ovra can carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arch {
    X86_64,
    Aarch64,
}

impl Arch {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::X86_64 => "x86_64",
            Self::Aarch64 => "aarch64",
        }
    }

    /// The build's file name among the app's resources.
    pub fn file_name(self) -> String {
        format!("ovra-daemon-{}-unknown-linux-musl", self.as_str())
    }

    /// The machine's answer to `uname -sm`, which must be exactly one of
    /// the two: it is the machine's own text.
    fn from_uname(said: &[u8]) -> Option<Self> {
        match said.strip_suffix(b"\n").unwrap_or(said) {
            b"Linux x86_64" => Some(Self::X86_64),
            b"Linux aarch64" => Some(Self::Aarch64),
            _ => None,
        }
    }
}

/// A SHA-256 as 64 lowercase hex digits: the only text that goes into the
/// install command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sha256(String);

impl Sha256 {
    pub fn parse(text: &str) -> Option<Self> {
        let hex = |c: u8| c.is_ascii_digit() || (b'a'..=b'f').contains(&c);
        (text.len() == 64 && text.bytes().all(hex)).then(|| Self(text.to_owned()))
    }

    fn of(bytes: &[u8]) -> Self {
        let digest = Hasher::digest(bytes);
        Self(digest.iter().map(|b| format!("{b:02x}")).collect())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A build of `ovra-daemon` whose bytes matched the hash fixed when the app
/// was built.
#[derive(Debug)]
pub struct Build {
    bytes: Vec<u8>,
    sha256: Sha256,
}

impl Build {
    /// `bytes` if they are at most [`UPLOAD_MAX`] long and hash to
    /// `expected`.
    pub fn checked(bytes: Vec<u8>, expected: &Sha256) -> Result<Self, String> {
        if bytes.len() > UPLOAD_MAX {
            return Err("This build of Ovra is too large to upload.".to_owned());
        }
        if &Sha256::of(&bytes) != expected {
            return Err("This build of Ovra does not match the one it was built with.".to_owned());
        }
        Ok(Self {
            bytes,
            sha256: expected.clone(),
        })
    }
}

/// The upload, on one line (csh takes no newline in a quoted string), for
/// `sh` whatever the person's login shell is. It refuses a data or `bin`
/// folder that is a symlink or someone else's, removes a planted `.new`
/// first, compares the hash itself (busybox's `sha256sum` has no
/// `--status`) and removes the `.new` after a short or failed upload, so
/// the binary there stays in place.
pub(crate) fn command(sha: &Sha256) -> String {
    format!(
        concat!(
            r#"sh -c 'umask 077; [ -n "$HOME" ] || exit 1; p="$HOME/.local/share/ovra"; d="$p/bin"; "#,
            r#"f="$d/ovra-daemon.new"; mkdir -p "$d" || exit 1; for x in "$p" "$d"; do [ -d "$x" ] "#,
            r#"&& [ ! -L "$x" ] && [ -O "$x" ] && chmod 700 "$x" || exit 1; done; "#,
            r#"[ ! -d "$d/ovra-daemon" ] || exit 1; rm -f "$f"; if cat > "$f" && "#,
            r#"[ "$(sha256sum < "$f" | cut -c1-64)" = "{}" ] && chmod 700 "$f" && "#,
            r#"mv -f "$f" "$d/ovra-daemon"; then exit 0; fi; rm -f "$f"; exit 1'"#,
        ),
        sha.as_str()
    )
}

impl OpenSsh {
    /// Installs this laptop's build on the machine `id`: asks its
    /// architecture, takes the build for it from `build_for` (an error is
    /// shown as given), uploads it while telling `progress` the bytes sent
    /// and the total, and points the machine's record at [`INSTALLED`].
    /// For the main window only, after the person's click.
    pub async fn install(
        &self,
        id: &HostId,
        build_for: impl FnOnce(Arch) -> Result<Build, String>,
        progress: impl Fn(u64, u64),
    ) -> Result<RemoteHost, Failure> {
        let uname = self.command(id, What::Uname).await?;
        let said = self.run(uname, None, UNAME_WAIT, &progress).await?;
        let Some(arch) = Arch::from_uname(&said) else {
            return Err(Failure::new(
                Reason::Exited,
                "Ovra can install itself only on Linux on x86_64 or aarch64",
            ));
        };
        let build = build_for(arch).map_err(|e| Failure::new(Reason::Exited, e))?;
        let upload = self.command(id, What::Install(&build.sha256)).await?;
        self.run(upload, Some(&build.bytes), UPLOAD_WAIT, &progress)
            .await
            .map_err(|f| match f.reason {
                // The command says nothing when it refuses.
                Reason::Exited if f.details.is_empty() => Failure::new(Reason::Exited, REFUSED),
                _ => f,
            })?;
        let installed = DaemonPath::parse(Some(INSTALLED))
            .map_err(|e| Failure::new(Reason::Exited, e.message))?;
        self.set_daemon(id, &installed)
            .map_err(|e| Failure::new(Reason::Exited, e.message))
    }

    /// Runs `command` with `input` on stdin, within `wait`, and returns the
    /// start of what it printed; a failure says why, from its exit status
    /// and stderr.
    async fn run(
        &self,
        mut command: tokio::process::Command,
        input: Option<&[u8]>,
        wait: Duration,
        progress: &impl Fn(u64, u64),
    ) -> Result<Vec<u8>, Failure> {
        command
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = tools::Group(command.spawn().map_err(|e| self.lost_ssh(&e))?);
        let stdin = child.0.stdin.take();
        let (stdout, stderr) = (child.0.stdout.take(), child.0.stderr.take());
        let feed = async move {
            let (Some(mut stdin), Some(bytes)) = (stdin, input) else {
                return;
            };
            let total = bytes.len() as u64;
            let mut sent = 0;
            for piece in bytes.chunks(PIECE) {
                // A machine that stops reading ends with its exit status.
                if stdin.write_all(piece).await.is_err() {
                    return;
                }
                sent += piece.len() as u64;
                progress(sent, total);
            }
            // Dropped here: the end of the upload.
        };
        let work = async {
            let ((), out, err) = tokio::join!(feed, kept(stdout), kept(stderr));
            let status = child.0.wait().await.ok();
            (status, out, err)
        };
        let Ok((status, out, err)) = tokio::time::timeout(wait, work).await else {
            return Err(Failure::new(
                Reason::Exited,
                "the remote machine took too long",
            ));
        };
        match status {
            Some(status) if status.success() => Ok(out),
            status => {
                let said = link::plain(&String::from_utf8_lossy(&err), SAID_MAX);
                Err(link::failed(status.and_then(|s| s.code()), said))
            }
        }
    }
}

/// The first [`SAID_MAX`] bytes of `pipe`, read to its end.
async fn kept(pipe: Option<impl AsyncRead + Unpin>) -> Vec<u8> {
    let mut kept = Vec::new();
    let Some(mut pipe) = pipe else {
        return kept;
    };
    let mut chunk = [0; 4096];
    while let Ok(n @ 1..) = pipe.read(&mut chunk).await {
        let room = SAID_MAX.saturating_sub(kept.len());
        kept.extend_from_slice(&chunk[..n.min(room)]);
    }
    kept
}

#[cfg(all(test, unix))]
mod tests;
