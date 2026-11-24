//! Each running session's netproxy (`docs/specs/security.md`): started
//! before the runtime, stopped when the session ends, and told when the
//! project's allowlist changes.

use crate::protocol::agents::SessionId;
use crate::protocol::netproxy::Allowlist;
use crate::protocol::projects::ProjectId;
use crate::services::{EventSink, NetReporter, ProfileNetwork, ProjectStore};

use super::Sessions;

type Result<T> = std::result::Result<T, crate::protocol::ProtocolError>;

impl Sessions {
    /// What every session may reach, whatever its project.
    pub(crate) fn net_base(&self) -> Allowlist {
        self.net.as_ref().map(|n| n.base()).unwrap_or_default()
    }

    /// Starts `session`'s proxy with `project`'s allowlist and returns the
    /// network for its profile: open without a netproxy.
    pub(crate) async fn net_start(
        &self,
        projects: &dyn ProjectStore,
        project: &ProjectId,
        session: &SessionId,
        sink: EventSink,
    ) -> Result<ProfileNetwork> {
        let Some(net) = &self.net else {
            return Ok(ProfileNetwork::Open);
        };
        let allow = projects.allowlist(project)?;
        let reporter = NetReporter::new(project.clone(), session.clone(), sink);
        let address = net.start(session.clone(), allow.clone(), reporter).await?;
        // The allowlist may have changed while the proxy started, and the
        // session may have ended: a running session gets the current list,
        // an ended one loses its proxy.
        let now = projects.allowlist(project);
        self.find(session, |s| {
            if s.info().status.is_ended() {
                net.stop(session);
            } else if now.as_ref().is_ok_and(|now| *now != allow) {
                net.update(session, now.unwrap_or_default());
            }
        })?;
        Ok(ProfileNetwork::Proxy { address })
    }

    /// Called under the sessions' lock when a session ends.
    pub(crate) fn net_stop(&self, session: &SessionId) {
        if let Some(net) = &self.net {
            net.stop(session);
        }
    }

    /// The project's allowlist was replaced: its running sessions follow.
    pub(crate) fn net_changed(&self, project: &ProjectId, allowlist: &Allowlist) {
        if let Some(net) = &self.net {
            for session in self.running(project) {
                net.update(&session, allowlist.clone());
            }
        }
    }
}
