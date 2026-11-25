//! Each running session's netproxy (`docs/specs/security.md`): started
//! before the runtime, stopped when the session ends, and told when the
//! project's allowlist changes.

use crate::protocol::agents::SessionId;
use crate::protocol::netproxy::Allowlist;
use crate::protocol::projects::ProjectId;
use crate::protocol::{ErrorCode, ProtocolError};
use crate::services::{EventSink, NetReporter, ProfileNetwork, ProjectStore, Secrets};

use super::{Sessions, lock};

type Result<T> = std::result::Result<T, ProtocolError>;

impl Sessions {
    /// What every session may reach, whatever its project.
    pub(crate) fn net_base(&self) -> Allowlist {
        self.net.as_ref().map(|n| n.base()).unwrap_or_default()
    }

    /// Starts the proxy of run `run` of `session` with `project`'s
    /// allowlist and returns the network for its profile: open without a
    /// netproxy. `conflict` if the session ended or started another run
    /// meanwhile; that run's proxy is left alone.
    pub(crate) async fn net_start(
        &self,
        projects: &dyn ProjectStore,
        secrets: &dyn Secrets,
        project: &ProjectId,
        session: &SessionId,
        run: u64,
        sink: EventSink,
    ) -> Result<ProfileNetwork> {
        let Some(net) = &self.net else {
            return Ok(ProfileNetwork::Open);
        };
        let allow = projects.allowlist(project)?;
        let core = lock(&self.ide).as_ref().map(|link| link.core.clone());
        let reporter = NetReporter::new(project.clone(), session.clone(), sink).with_core(core);
        // The bindings, never values: the proxy reads a value when it
        // adds it.
        let bound = secrets.list(project)?;
        let started = net
            .start(session.clone(), allow.clone(), reporter, bound)
            .await?;
        // The allowlist may have changed while the proxy started, and the
        // session may have ended: a running session gets the stored list,
        // an ended one loses its proxy.
        let _held = lock(&self.allowlists);
        let now = projects.allowlist(project);
        let current = self.find(session, |s| {
            let ours = s.run() == run;
            let current = ours && !s.info().status.is_ended();
            // A later run's proxy is left alone: ending this run already
            // stopped this one's.
            if ours && !current {
                net.stop(session);
            } else if current
                && let Ok(now) = &now
                && *now != allow
            {
                net.update(session, now.clone());
            }
            current
        })?;
        if !current {
            return Err(ProtocolError::new(
                ErrorCode::Conflict,
                "the session ended while its netproxy started",
            ));
        }
        now?;
        Ok(ProfileNetwork::Proxy {
            address: started.address,
            ca_bundle: started.ca_bundle,
        })
    }

    /// Called under the sessions' lock when a session ends.
    pub(crate) fn net_stop(&self, session: &SessionId) {
        if let Some(net) = &self.net {
            net.stop(session);
        }
    }

    /// Stores the project's allowlist and passes it to its running
    /// sessions' proxies, as one step against other changes.
    pub(crate) fn set_allowlist(
        &self,
        projects: &dyn ProjectStore,
        project: &ProjectId,
        allowlist: &Allowlist,
    ) -> Result<()> {
        let _held = lock(&self.allowlists);
        projects.set_allowlist(project, allowlist)?;
        if let Some(net) = &self.net {
            for session in self.running(project) {
                net.update(&session, allowlist.clone());
            }
        }
        Ok(())
    }
}
