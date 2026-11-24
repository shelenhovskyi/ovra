//! What one session's proxy lets through.

use std::sync::Arc;

use ovra_core::protocol::netproxy::{AllowedHost, Allowlist};

use crate::url::Authority;

/// One allowed host. Without a port it allows 443 and 80.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Rule {
    pub(crate) host: String,
    pub(crate) port: Option<u16>,
    /// An endpoint the user configured where Ovra runs, which may be on
    /// this machine or the local network.
    pub(crate) private_ok: bool,
}

impl Rule {
    pub(crate) fn named(host: &str) -> Self {
        Self {
            host: host.to_owned(),
            port: None,
            private_ok: false,
        }
    }

    /// Exactly the host and port of a URL the user set.
    pub(crate) fn configured(url: Authority) -> Self {
        let default = if url.scheme == "http" { 80 } else { 443 };
        Self {
            host: url.host,
            port: Some(url.port.unwrap_or(default)),
            private_ok: true,
        }
    }

    fn allows(&self, host: &str, port: u16) -> bool {
        self.host == host
            && match self.port {
                Some(p) => p == port,
                None => port == 443 || port == 80,
            }
    }
}

/// The base rules plus the project's hosts.
#[derive(Debug, Clone)]
pub(crate) struct Rules {
    pub(crate) base: Arc<[Rule]>,
    pub(crate) project: Vec<AllowedHost>,
}

/// What the rules say about a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// Allowed; `private_ok` lets it reach a private address.
    Allowed { private_ok: bool },
    /// The host is not listed at all.
    NotListed,
    /// The host is listed, but not with this port.
    WrongPort,
}

impl Rules {
    /// `host` as [`crate::url::host_port`] normalizes it.
    pub(crate) fn verdict(&self, host: &str, port: u16) -> Verdict {
        let project = self.project.iter().map(|h| Rule {
            host: h.host.clone(),
            port: h.port,
            private_ok: false,
        });
        let mut listed = false;
        for rule in self.base.iter().cloned().chain(project) {
            if rule.allows(host, port) {
                return Verdict::Allowed {
                    private_ok: rule.private_ok,
                };
            }
            listed |= rule.host == host;
        }
        if listed {
            Verdict::WrongPort
        } else {
            Verdict::NotListed
        }
    }
}

pub(crate) fn as_allowlist(rules: &[Rule]) -> Allowlist {
    Allowlist {
        hosts: rules
            .iter()
            .map(|r| AllowedHost {
                host: r.host.clone(),
                port: r.port,
            })
            .collect(),
    }
}
