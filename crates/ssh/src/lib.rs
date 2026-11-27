//! Remote machines over the person's own OpenSSH (ADR 0016,
//! `docs/specs/ssh-remotes.md`). Security code: every change here gets an
//! independent security review.
//!
//! The stub holds the command line; hosts, pins, `ssh -G` checks and the
//! frame link follow in their own steps.

use std::path::Path;

use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::ssh::{DaemonPath, Destination, HostId};

/// The arguments for `ssh` (without the program) that reach `daemon` on
/// `destination`, trusting only the keys in `pins` under `ovra-<id>`.
///
/// OpenSSH takes the first value it sees for an option and reads the
/// command line before `~/.ssh/config`, so each of these wins over the
/// person's config. Everything else in their config still applies.
pub fn arguments(
    pins: &Path,
    id: &HostId,
    destination: &Destination,
    daemon: &DaemonPath,
) -> Result<Vec<String>, ProtocolError> {
    let pins = option_path(pins)?;
    let mut args: Vec<String> = ["-T", "-x", "-a"].map(String::from).to_vec();
    let options = [
        // Must come before the option a stock OpenSSH does not know.
        "IgnoreUnknown=GSSAPIKeyExchange".to_owned(),
        "GSSAPIKeyExchange=no".to_owned(),
        "BatchMode=yes".to_owned(),
        "ConnectTimeout=20".to_owned(),
        "StrictHostKeyChecking=yes".to_owned(),
        "UpdateHostKeys=no".to_owned(),
        format!("HostKeyAlias=ovra-{}", id.as_str()),
        format!("UserKnownHostsFile={pins}"),
        "GlobalKnownHostsFile=none".to_owned(),
        "KnownHostsCommand=none".to_owned(),
        "VerifyHostKeyDNS=no".to_owned(),
        "CheckHostIP=no".to_owned(),
        "ForwardAgent=no".to_owned(),
        "ForwardX11=no".to_owned(),
        "ForwardX11Trusted=no".to_owned(),
        "GSSAPIDelegateCredentials=no".to_owned(),
        "ClearAllForwardings=yes".to_owned(),
        "Tunnel=no".to_owned(),
        "PermitLocalCommand=no".to_owned(),
        "RemoteCommand=none".to_owned(),
        "RequestTTY=no".to_owned(),
        "SessionType=default".to_owned(),
        "StdinNull=no".to_owned(),
        "ForkAfterAuthentication=no".to_owned(),
        "EscapeChar=none".to_owned(),
        "ControlMaster=no".to_owned(),
        "ControlPath=none".to_owned(),
        // Only replaces a SetEnv in the person's config; nothing reads it.
        "SetEnv=OVRA_UNUSED=1".to_owned(),
        "ServerAliveInterval=15".to_owned(),
        "ServerAliveCountMax=4".to_owned(),
    ];
    for option in options {
        args.push("-o".to_owned());
        args.push(option);
    }
    args.push("--".to_owned());
    args.push(destination.as_str().to_owned());
    args.push(daemon.as_str().to_owned());
    args.push("--stdio".to_owned());
    Ok(args)
}

/// A path as one value of a list option: quoted, because
/// `UserKnownHostsFile` splits on spaces and backslash escapes (every
/// Windows path has them), with `\` and `%` doubled, because OpenSSH
/// unescapes the one and expands `%` tokens. `$` has no escape (OpenSSH
/// expands `${NAME}`), so a path with one is refused.
fn option_path(path: &Path) -> Result<String, ProtocolError> {
    let text = path
        .to_str()
        .filter(|text| path.is_absolute() && !text.contains(['"', '$', '\n', '\r', '\0']));
    let Some(text) = text else {
        return Err(ProtocolError::new(
            ErrorCode::Unavailable,
            "Ovra's data folder has a name SSH cannot be given safely",
        ));
    };
    Ok(format!(
        "\"{}\"",
        text.replace('\\', "\\\\").replace('%', "%%")
    ))
}

#[cfg(test)]
mod tests;
