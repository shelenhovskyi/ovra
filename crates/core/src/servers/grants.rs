//! Loopback grants of sessions (ADR 0008): a session may reach the ready
//! servers of its own work dir, and only while that generation runs.
//! Grants are recomputed whenever a server becomes ready, ends or stops,
//! and when a session starts or ends.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{Place, State, defs};
use crate::Core;
use crate::protocol::agents::SessionId;
use crate::protocol::audit::AuditEvent;
use crate::protocol::projects::ProjectId;
use crate::protocol::servers::{ServerId, ServerStatus};
use crate::services::{LoopbackGrant, NetProxy, SandboxStatus};

/// One grant as the audit log names it: server, port, generation.
pub(super) type Granted = (ServerId, u16, u64);

/// Recomputes the grants of every running session in `place`'s work dir.
pub(super) fn regrant(core: &Core, state: &mut State, place: &Place) {
    let Some(net) = core.dev_servers().net.clone() else {
        return;
    };
    for session in core.sessions().running(&place.project) {
        if work_dir(core, &session).as_deref() == Some(place.dir.as_path()) {
            apply(
                core,
                net.as_ref(),
                state,
                &session,
                &place.project,
                &place.dir,
                false,
            );
        }
    }
}

/// Gives a session whose netproxy just started the servers of its work dir.
pub(crate) fn session_started(core: &Core, session: &SessionId) {
    let Some(net) = core.dev_servers().net.clone() else {
        return;
    };
    let found = core.sessions().find(session, |s| {
        let info = s.info();
        let dir = s.launch().map(|l| l.cwd);
        (info.project, dir.filter(|_| !info.status.is_ended()))
    });
    if let Ok((project, Some(dir))) = found {
        let mut state = core.dev_servers().lock();
        apply(
            core,
            net.as_ref(),
            &mut state,
            session,
            &project,
            &dir,
            true,
        );
    }
}

/// Forgets an ended session's grants; its netproxy stopped with it.
pub(crate) fn session_ended(core: &Core, session: &SessionId) {
    let Ok(project) = core.sessions().find(session, |s| s.info().project) else {
        return;
    };
    let old = core.dev_servers().lock().granted.remove(session);
    for (server, port, generation) in old.unwrap_or_default() {
        let event = AuditEvent::LoopbackRevoked {
            server,
            port,
            generation,
        };
        crate::audit::record(core, &project, Some(session), event);
    }
}

/// The ports defined in `dir` when a session starts there, which its
/// sandbox forwards to its netproxy. None while servers cannot run.
pub(crate) async fn forward(core: &Core, dir: &Path) -> Vec<u16> {
    if let SandboxStatus::Unavailable { .. } = core.dev_servers().dev.status() {
        return Vec::new();
    }
    defs::read(core.fs_service(), dir)
        .await
        .map(|found| {
            found
                .iter()
                .map(|d| d.port)
                .filter(|p| defs::server_port(*p))
                .collect()
        })
        .unwrap_or_default()
}

fn work_dir(core: &Core, session: &SessionId) -> Option<PathBuf> {
    core.sessions()
        .find(session, |s| s.launch().map(|l| l.cwd))
        .ok()
        .flatten()
}

/// Grants `session` the ready servers of `dir`, recording what changed.
/// `force` passes them on even if unchanged: a new netproxy has none.
fn apply(
    core: &Core,
    net: &dyn NetProxy,
    state: &mut State,
    session: &SessionId,
    project: &ProjectId,
    dir: &Path,
    force: bool,
) {
    let mut new: Vec<Granted> = state
        .servers
        .iter()
        .filter(|e| &e.place.project == project && e.place.dir == dir)
        .filter(|e| e.status == ServerStatus::Ready && defs::server_port(e.port))
        .map(|e| (e.id.clone(), e.port, e.generation))
        .collect();
    new.sort_by_key(|g| g.1);
    let old = state.granted.get(session).cloned().unwrap_or_default();
    if new == old && (!force || new.is_empty()) {
        return;
    }
    let audit = |event| crate::audit::record(core, project, Some(session), event);
    for (server, port, generation) in old.iter().filter(|g| !new.contains(g)).cloned() {
        audit(AuditEvent::LoopbackRevoked {
            server,
            port,
            generation,
        });
    }
    for (server, port, generation) in new.iter().filter(|g| !old.contains(g)).cloned() {
        audit(AuditEvent::LoopbackGranted {
            server,
            port,
            generation,
        });
    }
    let dev = core.dev_servers().dev.clone();
    let grants = new
        .iter()
        .map(|(server, port, generation)| {
            let (dev, dir, port) = (dev.clone(), dir.to_path_buf(), *port);
            LoopbackGrant {
                server: server.clone(),
                port,
                generation: *generation,
                dial: Arc::new(move || {
                    let (dev, dir) = (dev.clone(), dir.clone());
                    Box::pin(async move { dev.connect(dir, port).await })
                }),
            }
        })
        .collect();
    net.grant(session, grants);
    if new.is_empty() {
        state.granted.remove(session);
    } else {
        state.granted.insert(session.clone(), new);
    }
}
