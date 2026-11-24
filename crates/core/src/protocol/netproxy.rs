//! `netproxy.*`: the hosts a project's agent sessions may reach through
//! their netproxy (`docs/specs/security.md`). Only the person changes them.

use serde::{Deserialize, Serialize};

use super::agents::SessionId;
use super::projects::ProjectId;

/// One host a session may reach: a lowercase DNS name or IP address,
/// without a trailing dot or brackets. Without a port it allows 443 and 80.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AllowedHost {
    pub host: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Allowlist {
    pub hosts: Vec<AllowedHost>,
}

/// What a project's sessions may reach.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectAllowlist {
    /// What every session may reach: what `claude` itself needs. Not editable.
    pub base: Allowlist,
    /// The project's own hosts, added to `base`.
    pub project: Allowlist,
    /// Hosts in `project` where anyone can receive data (code hosts, file
    /// shares, cloud storage): an agent can send the project out through
    /// them. The UI warns about each.
    pub broad: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum NetproxyRequest {
    /// Answers `netproxy.allowlist`.
    #[serde(rename = "netproxy.get_allowlist")]
    GetAllowlist { project: ProjectId },
    /// Replaces the project's own hosts; running sessions follow at once
    /// for new connections. `bad_request` for a host that is not a plain
    /// name or address, a wildcard, port 0, or an address that is never
    /// reachable (loopback, private, link-local). Answers
    /// `netproxy.allowlist` with what was kept, normalized.
    #[serde(rename = "netproxy.set_allowlist")]
    SetAllowlist {
        project: ProjectId,
        allowlist: Allowlist,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum NetproxyResponse {
    #[serde(rename = "netproxy.allowlist")]
    Allowlist(ProjectAllowlist),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum NetproxyEvent {
    /// A project's own hosts were replaced.
    #[serde(rename = "netproxy.changed")]
    Changed { project: ProjectId },
    /// A session's netproxy refused a connection.
    #[serde(rename = "netproxy.refused")]
    Refused {
        project: ProjectId,
        session: SessionId,
        /// `host:port` as the agent asked for it.
        host: String,
        reason: String,
    },
}
