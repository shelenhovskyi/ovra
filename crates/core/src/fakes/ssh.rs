use std::collections::BTreeMap;
use std::sync::Mutex;

use crate::protocol::ssh::{HostKey, PreparedHost, RemoteHost};
use crate::protocol::{ErrorCode, ProtocolError};
use crate::services::{BoxFuture, RemoteHosts, Result};
use crate::ssh::{DaemonPath, Destination, HostId, MachineName};

use super::lock;

/// Remote machines in memory. `known` stands in for the person's
/// `known_hosts`: the keys it has for each destination.
#[derive(Debug, Default)]
pub struct MemoryRemoteHosts {
    pub known: Mutex<BTreeMap<String, Vec<HostKey>>>,
    hosts: Mutex<Vec<RemoteHost>>,
}

impl MemoryRemoteHosts {
    /// One whose `known_hosts` trusts `destination` with `keys`.
    pub fn knowing(destination: &str, keys: &[(&str, &str)]) -> Self {
        let keys = keys
            .iter()
            .map(|(kind, fingerprint)| HostKey {
                kind: (*kind).into(),
                fingerprint: (*fingerprint).into(),
            })
            .collect();
        Self {
            known: Mutex::new(BTreeMap::from([(destination.to_owned(), keys)])),
            ..Self::default()
        }
    }
}

fn missing(id: &HostId) -> ProtocolError {
    ProtocolError::new(
        ErrorCode::NotFound,
        format!("no remote machine {}", id.as_str()),
    )
}

impl RemoteHosts for MemoryRemoteHosts {
    fn list(&self) -> Result<Vec<RemoteHost>> {
        Ok(lock(&self.hosts).clone())
    }

    fn prepare<'a>(&'a self, destination: &'a Destination) -> BoxFuture<'a, Result<PreparedHost>> {
        Box::pin(async move { self.prepared(destination) })
    }

    fn add<'a>(
        &'a self,
        name: &'a MachineName,
        destination: &'a Destination,
        daemon: &'a DaemonPath,
        fingerprints: &'a [String],
    ) -> BoxFuture<'a, Result<RemoteHost>> {
        Box::pin(async move { self.added(name, destination, daemon, fingerprints) })
    }

    fn remove(&self, id: &HostId) -> Result<()> {
        let mut hosts = lock(&self.hosts);
        let before = hosts.len();
        hosts.retain(|h| h.id != id.as_str());
        if hosts.len() == before {
            return Err(missing(id));
        }
        Ok(())
    }

    fn rename(&self, id: &HostId, name: &MachineName) -> Result<RemoteHost> {
        let mut hosts = lock(&self.hosts);
        let host = hosts
            .iter_mut()
            .find(|h| h.id == id.as_str())
            .ok_or_else(|| missing(id))?;
        host.name = name.as_str().into();
        Ok(host.clone())
    }
}

impl MemoryRemoteHosts {
    fn prepared(&self, destination: &Destination) -> Result<PreparedHost> {
        let keys = lock(&self.known)
            .get(destination.as_str())
            .cloned()
            .unwrap_or_default();
        let (user, host) = destination
            .as_str()
            .split_once('@')
            .unwrap_or(("me", destination.as_str()));
        Ok(PreparedHost {
            destination: destination.as_str().into(),
            host_name: host.into(),
            port: 22,
            user: user.into(),
            key_name: host.into(),
            keys,
        })
    }

    fn added(
        &self,
        name: &MachineName,
        destination: &Destination,
        daemon: &DaemonPath,
        fingerprints: &[String],
    ) -> Result<RemoteHost> {
        let known = lock(&self.known)
            .get(destination.as_str())
            .cloned()
            .unwrap_or_default();
        if !fingerprints
            .iter()
            .all(|f| known.iter().any(|k| &k.fingerprint == f))
        {
            return Err(ProtocolError::new(
                ErrorCode::Conflict,
                "your SSH no longer trusts these keys for this machine",
            ));
        }
        let mut hosts = lock(&self.hosts);
        let host = RemoteHost {
            id: format!("h{}", hosts.len() + 1),
            name: name.as_str().into(),
            destination: destination.as_str().into(),
            daemon: daemon.as_str().into(),
            fingerprints: fingerprints.to_vec(),
            last_connected: None,
        };
        hosts.push(host.clone());
        Ok(host)
    }
}
