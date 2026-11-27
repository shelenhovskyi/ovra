//! `ssh.*`: remote machines over the person's own SSH (ADR 0016,
//! `docs/specs/ssh-remotes.md`). The service keeps hosts and pins; the core
//! checks every value first, because a destination and a daemon path end up
//! on an `ssh` command line and in the remote machine's shell.

use crate::Core;
use crate::protocol::ssh::{SshEvent, SshRequest, SshResponse};
use crate::protocol::{ErrorCode, OkResponse, ProtocolError, Response};

#[cfg(test)]
mod tests;

/// `ovra-daemon` when the person names no path.
const DEFAULT_DAEMON: &str = "ovra-daemon";

/// The longest user or host part of a destination.
const PART_MAX: usize = 64;
/// The longest name of a machine, in characters.
const NAME_MAX: usize = 40;
/// The longest daemon path.
const DAEMON_MAX: usize = 255;
/// The most keys one machine can pin.
const KEYS_MAX: usize = 8;

/// The id the service gives a machine: `[a-z0-9]{1,32}`. It names the
/// machine's pin folder and its `HostKeyAlias`, so nothing else may appear.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostId(String);

impl HostId {
    pub fn parse(text: &str) -> Result<Self, ProtocolError> {
        let ok = (1..=32).contains(&text.len())
            && text
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
        if !ok {
            return Err(invalid("no such remote machine"));
        }
        Ok(Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A host alias or `user@host`, as typed in a terminal. Each part is
/// `[A-Za-z0-9._-]+`, at most 64 characters, and starts with neither `-`
/// (an `ssh` option) nor `.`. No port and no `ssh://`: a port belongs in
/// `~/.ssh/config`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Destination(String);

impl Destination {
    pub fn parse(text: &str) -> Result<Self, ProtocolError> {
        let (user, host) = match text.split_once('@') {
            Some((user, host)) => (Some(user), host),
            None => (None, text),
        };
        if user.is_some_and(|user| !part(user)) || !part(host) {
            return Err(invalid(
                "type a host name from your SSH config, or user@host (letters, digits, '.', '_', '-')",
            ));
        }
        Ok(Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn part(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= PART_MAX
        && !text.starts_with(['-', '.'])
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// Where `ovra-daemon` is on the remote machine: a bare name, an absolute
/// path or a path starting with `~/` or `./`, made of `[A-Za-z0-9._/-]`
/// after that prefix, with no `.` or `..` segment (a bare `.` would be the
/// shell's `source`). The remote shell runs it, so nothing else may appear.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonPath(String);

impl DaemonPath {
    pub fn parse(text: Option<&str>) -> Result<Self, ProtocolError> {
        let text = text.unwrap_or(DEFAULT_DAEMON);
        let rest = text
            .strip_prefix("~/")
            .or_else(|| text.strip_prefix("./"))
            .unwrap_or(text);
        let ok = !rest.is_empty()
            && text.len() <= DAEMON_MAX
            && !rest.starts_with('-')
            && !rest.ends_with('/')
            && rest
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'/'))
            && rest.split('/').skip(1).all(|segment| !segment.is_empty())
            && rest
                .split('/')
                .all(|segment| segment != ".." && segment != ".");
        if !ok {
            return Err(invalid(
                "the path of Ovra on the machine may use only letters, digits, '.', '_', '-' and '/', and may start with '~/' or './'",
            ));
        }
        Ok(Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What the person calls a machine: 1 to 40 characters, trimmed, with no
/// control, format (bidi) or line characters, since it is shown in title
/// bars and notifications.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineName(String);

impl MachineName {
    pub fn parse(text: &str) -> Result<Self, ProtocolError> {
        let text = text.trim();
        let count = text.chars().count();
        let shows = text.chars().any(|c| !c.is_whitespace() && !blank(c));
        if !shows || count > NAME_MAX || text.chars().any(hidden) {
            return Err(invalid("name the machine with 1 to 40 visible characters"));
        }
        Ok(Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Control characters, bidi and other invisible format characters, line
/// or paragraph separators, tag characters, variation selectors and
/// private-use characters.
fn hidden(c: char) -> bool {
    c.is_control()
        || matches!(c,
            '\u{00AD}' | '\u{034F}' | '\u{061C}' | '\u{180B}'..='\u{180F}'
            | '\u{200B}'..='\u{200F}' | '\u{2028}'..='\u{202E}' | '\u{2060}'..='\u{206F}'
            | '\u{E000}'..='\u{F8FF}' | '\u{FE00}'..='\u{FE0F}' | '\u{FEFF}'
            | '\u{FFF9}'..='\u{FFFB}' | '\u{E0000}'..='\u{E007F}' | '\u{E0100}'..='\u{E01EF}'
            | '\u{F0000}'..)
}

/// Characters that take space but show nothing: Hangul fillers and the
/// Braille blank. Allowed beside visible ones, never alone.
fn blank(c: char) -> bool {
    matches!(
        c,
        '\u{115F}' | '\u{1160}' | '\u{2800}' | '\u{3164}' | '\u{FFA0}'
    )
}

/// What a machine may pin: 1 to [`KEYS_MAX`] different fingerprints.
/// `ovra-ssh` checks its saved list with this as well.
pub fn fingerprints_ok(fingerprints: &[String]) -> bool {
    !fingerprints.is_empty()
        && fingerprints.len() <= KEYS_MAX
        && fingerprints.iter().all(|f| fingerprint(f))
        && (1..fingerprints.len()).all(|i| !fingerprints[..i].contains(&fingerprints[i]))
}

/// `SHA256:` and unpadded base64, as `ssh-keygen -l` prints it.
fn fingerprint(text: &str) -> bool {
    text.strip_prefix("SHA256:").is_some_and(|b64| {
        b64.len() == 43
            && b64
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/'))
    })
}

fn invalid(message: &str) -> ProtocolError {
    ProtocolError::new(ErrorCode::BadRequest, message)
}

pub(crate) async fn handle(core: &Core, request: SshRequest) -> Result<Response, ProtocolError> {
    let ssh = core.ssh();
    let changed = |core: &Core| -> Result<(), ProtocolError> {
        core.emit(SshEvent::Changed(core.ssh().list()?));
        Ok(())
    };
    Ok(match request {
        SshRequest::List => Response::Ssh(SshResponse::Hosts(ssh.list()?)),
        SshRequest::Prepare { destination } => {
            let destination = Destination::parse(&destination)?;
            Response::Ssh(SshResponse::Prepared(ssh.prepare(&destination).await?))
        }
        SshRequest::Add(new) => {
            let name = MachineName::parse(&new.name)?;
            let destination = Destination::parse(&new.destination)?;
            let daemon = DaemonPath::parse(new.daemon.as_deref())?;
            if !fingerprints_ok(&new.fingerprints) {
                return Err(invalid("pin the keys that were shown for this machine"));
            }
            let host = ssh
                .add(&name, &destination, &daemon, &new.fingerprints)
                .await?;
            changed(core)?;
            Response::Ssh(SshResponse::Host(host))
        }
        SshRequest::Remove { id } => {
            ssh.remove(&HostId::parse(&id)?)?;
            changed(core)?;
            Response::Ok(OkResponse::Ok)
        }
        SshRequest::Rename { id, name } => {
            let host = ssh.rename(&HostId::parse(&id)?, &MachineName::parse(&name)?)?;
            changed(core)?;
            Response::Ssh(SshResponse::Host(host))
        }
    })
}
