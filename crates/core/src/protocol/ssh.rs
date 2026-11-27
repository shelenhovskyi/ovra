//! `ssh.*`: the remote machines this computer connects to over the
//! person's own SSH (ADR 0016, `docs/specs/ssh-remotes.md`). Only the
//! laptop's core answers these; a phone never may, and the bridge drops
//! any `ssh.*` frame a remote machine sends.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum SshRequest {
    #[serde(rename = "ssh.list")]
    List,
    /// What `ssh -G` resolves for `destination` and the keys the person's
    /// own `known_hosts` already trusts for it. Makes no SSH connection.
    #[serde(rename = "ssh.prepare")]
    Prepare { destination: String },
    /// Pins exactly `fingerprints`, which `ssh.prepare` returned and the
    /// person saw.
    #[serde(rename = "ssh.add")]
    Add(NewRemoteHost),
    #[serde(rename = "ssh.remove")]
    Remove { id: String },
    #[serde(rename = "ssh.rename")]
    Rename { id: String, name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum SshResponse {
    #[serde(rename = "ssh.hosts")]
    Hosts(Vec<RemoteHost>),
    #[serde(rename = "ssh.prepared")]
    Prepared(PreparedHost),
    #[serde(rename = "ssh.host")]
    Host(RemoteHost),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum SshEvent {
    /// The list of remote machines changed (added, removed, renamed).
    #[serde(rename = "ssh.changed")]
    Changed(Vec<RemoteHost>),
}

/// A remote machine the person added.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteHost {
    pub id: String,
    /// What the person calls it; shown as "on <name>".
    pub name: String,
    /// As typed in a terminal: a host alias or `user@host`.
    pub destination: String,
    /// `ovra-daemon` on the remote machine: a bare name, an absolute path
    /// or a path starting with `~/`.
    pub daemon: String,
    /// SHA256 fingerprints of the pinned host keys.
    pub fingerprints: Vec<String>,
    /// Milliseconds since the Unix epoch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_connected: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewRemoteHost {
    pub name: String,
    pub destination: String,
    /// `ovra-daemon` when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daemon: Option<String>,
    pub fingerprints: Vec<String>,
}

/// What `ssh -G` resolves for a destination, and the keys found for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreparedHost {
    pub destination: String,
    pub host_name: String,
    pub port: u16,
    pub user: String,
    /// The name `ssh` checks keys under (`HostKeyAlias`, or the host name,
    /// or `[host]:port`).
    pub key_name: String,
    /// Empty: the person connects once in a terminal first.
    pub keys: Vec<HostKey>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostKey {
    /// E.g. `ssh-ed25519`.
    pub kind: String,
    /// `SHA256:...`.
    pub fingerprint: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{Event, Request, Response};

    #[test]
    fn wire_format_is_stable() {
        let host = RemoteHost {
            id: "h1".into(),
            name: "Build box".into(),
            destination: "dev@build".into(),
            daemon: "ovra-daemon".into(),
            fingerprints: vec!["SHA256:abc".into()],
            last_connected: Some(1),
        };
        insta::assert_json_snapshot!(serde_json::json!({
            "requests": [
                Request::Ssh(SshRequest::List),
                Request::Ssh(SshRequest::Prepare { destination: "build".into() }),
                Request::Ssh(SshRequest::Add(NewRemoteHost {
                    name: "Build box".into(),
                    destination: "dev@build".into(),
                    daemon: None,
                    fingerprints: vec!["SHA256:abc".into()],
                })),
                Request::Ssh(SshRequest::Remove { id: "h1".into() }),
                Request::Ssh(SshRequest::Rename { id: "h1".into(), name: "Box".into() }),
            ],
            "responses": [
                Response::Ssh(SshResponse::Hosts(vec![host.clone()])),
                Response::Ssh(SshResponse::Prepared(PreparedHost {
                    destination: "build".into(),
                    host_name: "build.example.org".into(),
                    port: 22,
                    user: "dev".into(),
                    key_name: "build.example.org".into(),
                    keys: vec![HostKey { kind: "ssh-ed25519".into(), fingerprint: "SHA256:abc".into() }],
                })),
                Response::Ssh(SshResponse::Host(host.clone())),
            ],
            "events": [Event::from(SshEvent::Changed(vec![host]))],
        }));
    }
}
