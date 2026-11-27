//! The host keys the person's own `known_hosts` already trusts. Ovra never
//! accepts a key itself; it copies these, after the person saw them.

use std::process::Command;

use ovra_core::protocol::ssh::HostKey;
use ovra_core::protocol::{ErrorCode, ProtocolError};

use crate::tools::Tools;

/// One trusted key: what the person is shown and the line Ovra pins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Known {
    pub(crate) key: HostKey,
    /// The key's base64 blob.
    pub(crate) blob: String,
}

/// The keys `files` trust for `name`, without revoked ones. A host known
/// only through a certificate authority is refused for now.
pub(crate) fn lookup(
    tools: &Tools,
    name: &str,
    files: &[String],
) -> Result<Vec<Known>, ProtocolError> {
    let mut keys: Vec<(String, String)> = Vec::new();
    let mut revoked: Vec<String> = Vec::new();
    let mut authority = false;
    for file in files {
        if !std::path::Path::new(file).is_file() {
            continue;
        }
        let out = tools.run(
            Command::new(&tools.keygen)
                .args(["-F", name, "-f"])
                .arg(file),
            None,
        )?;
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            match parse(line) {
                Some(Line::Key(kind, blob)) => keys.push((kind, blob)),
                Some(Line::Revoked(blob)) => revoked.push(blob),
                Some(Line::Authority) => authority = true,
                None => {}
            }
        }
    }
    keys.retain(|(_, blob)| !revoked.contains(blob));
    keys.dedup();
    if keys.is_empty() && authority {
        return Err(ProtocolError::new(
            ErrorCode::Unavailable,
            "this machine is trusted through an SSH certificate authority, which Ovra does not support yet",
        ));
    }
    fingerprints(tools, keys)
}

enum Line {
    Key(String, String),
    Revoked(String),
    Authority,
}

/// One `known_hosts` line: `[@marker] hosts type blob [comment]`.
fn parse(line: &str) -> Option<Line> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let mut parts = line.split_whitespace();
    let first = parts.next()?;
    let marker = first.starts_with('@').then_some(first);
    if marker.is_some() {
        parts.next()?;
    }
    let kind = parts.next()?;
    let blob = parts.next()?;
    let plain = |s: &str, extra: &[u8]| {
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || extra.contains(&b))
    };
    if !plain(kind, b"-@.") || !plain(blob, b"+/=") {
        return None;
    }
    match marker {
        None => Some(Line::Key(kind.to_owned(), blob.to_owned())),
        Some("@revoked") => Some(Line::Revoked(blob.to_owned())),
        Some("@cert-authority") => Some(Line::Authority),
        Some(_) => None,
    }
}

/// Fingerprints from `ssh-keygen -l`, in the same order as `keys`.
fn fingerprints(tools: &Tools, keys: Vec<(String, String)>) -> Result<Vec<Known>, ProtocolError> {
    if keys.is_empty() {
        return Ok(Vec::new());
    }
    let input: String = keys
        .iter()
        .map(|(kind, blob)| format!("x {kind} {blob}\n"))
        .collect();
    let out = tools.run(
        Command::new(&tools.keygen).args(["-l", "-E", "sha256", "-f", "-"]),
        Some(&input),
    )?;
    let text = String::from_utf8_lossy(&out.stdout);
    let prints: Vec<&str> = text
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1))
        .collect();
    if prints.len() != keys.len() || !prints.iter().all(|p| p.starts_with("SHA256:")) {
        return Err(ProtocolError::new(
            ErrorCode::Internal,
            "ssh-keygen could not read the keys in your known_hosts",
        ));
    }
    Ok(keys
        .into_iter()
        .zip(prints)
        .map(|((kind, blob), fingerprint)| Known {
            key: HostKey {
                kind,
                fingerprint: fingerprint.to_owned(),
            },
            blob,
        })
        .collect())
}
