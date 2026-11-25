//! Loopback grants of sessions (ADR 0008): a session may reach the ready
//! servers of its own work dir, and only while that generation runs.
//! Grants are recomputed whenever a server becomes ready, ends or stops,
//! and when a session starts or ends.
//!
//! Each run of a session (a resume is a new one) has a netproxy of its
//! own, so what was granted is kept per run: the end of an earlier run,
//! reported late, never clears what a later run's netproxy holds. And a
//! dial checks that its server still runs that generation, so a grant a
//! netproxy kept by mistake reaches nothing.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{Place, State, defs};
use crate::Core;
use crate::protocol::agents::SessionId;
use crate::protocol::audit::AuditEvent;
use crate::protocol::projects::ProjectId;
use crate::protocol::servers::{ServerId, ServerStatus};
use crate::protocol::{ErrorCode, ProtocolError};
use crate::services::{LoopbackGrant, NetProxy, PodDial, SandboxStatus};

/// One grant as the audit log names it: server, port, generation.
pub(super) type Granted = (ServerId, u16, u64);

/// What one run's netproxy was granted last.
#[derive(Debug)]
pub(super) struct Given {
    run: u64,
    grants: Vec<Granted>,
}

/// Recomputes the grants of every running session in `place`'s work dir.
pub(super) fn regrant(core: &Core, state: &mut State, place: &Place) {
    let Some(net) = core.dev_servers().net.clone() else {
        return;
    };
    for session in core.sessions().running(&place.project) {
        let Some((dir, run)) = launched(core, &session) else {
            continue;
        };
        if dir == place.dir {
            let to = To {
                session: &session,
                project: &place.project,
                dir: &place.dir,
                run,
            };
            apply(core, net.as_ref(), state, &to, false);
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
        (
            info.project,
            dir.filter(|_| !info.status.is_ended()),
            s.run(),
        )
    });
    if let Ok((project, Some(dir), run)) = found {
        let mut state = core.dev_servers().lock();
        let to = To {
            session,
            project: &project,
            dir: &dir,
            run,
        };
        apply(core, net.as_ref(), &mut state, &to, true);
    }
}

/// Forgets the grants of a session's ended `run`; its netproxy stopped with
/// it. Those of a later run stay.
pub(super) fn session_ended(core: &Core, session: &SessionId, run: u64) {
    let Ok(project) = core.sessions().find(session, |s| s.info().project) else {
        return;
    };
    let old = {
        let mut state = core.dev_servers().lock();
        match state.granted.get(session) {
            Some(given) if given.run <= run => state.granted.remove(session),
            _ => None,
        }
    };
    for (server, port, generation) in old.map(|g| g.grants).unwrap_or_default() {
        let event = AuditEvent::LoopbackRevoked {
            server,
            port,
            generation,
        };
        crate::audit::record(core, &project, Some(session), event);
    }
}

/// The ports of the servers `session` may reach now.
pub(crate) fn granted_ports(core: &Core, session: &SessionId) -> Vec<u16> {
    let state = core.dev_servers().lock();
    let granted = state
        .granted
        .get(session)
        .map(|given| given.grants.as_slice());
    granted.unwrap_or_default().iter().map(|g| g.1).collect()
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

/// A session's work dir and current run.
fn launched(core: &Core, session: &SessionId) -> Option<(PathBuf, u64)> {
    core.sessions()
        .find(session, |s| s.launch().map(|l| (l.cwd, s.run())))
        .ok()
        .flatten()
}

/// The run of a session whose grants change.
struct To<'a> {
    session: &'a SessionId,
    project: &'a ProjectId,
    dir: &'a Path,
    run: u64,
}

/// Grants `to.session` the ready servers of `to.dir`, recording what
/// changed. `force` passes them on even if unchanged: a new netproxy has
/// none. Without a record for this run they are passed on too.
fn apply(core: &Core, net: &dyn NetProxy, state: &mut State, to: &To<'_>, force: bool) {
    let given = state.granted.get(to.session);
    if given.is_some_and(|g| g.run > to.run) {
        return;
    }
    let mut new: Vec<Granted> = state
        .servers
        .iter()
        .filter(|e| &e.place.project == to.project && e.place.dir == to.dir)
        .filter(|e| e.status == ServerStatus::Ready && defs::server_port(e.port))
        .map(|e| (e.id.clone(), e.port, e.generation))
        .collect();
    new.sort_by_key(|g| g.1);
    let audit = |event| crate::audit::record(core, to.project, Some(to.session), event);
    let (known, old) = match given {
        Some(g) if g.run == to.run => (true, g.grants.clone()),
        Some(g) => {
            // An earlier run's netproxy ended, and its grants with it.
            for (server, port, generation) in g.grants.iter().cloned() {
                audit(AuditEvent::LoopbackRevoked {
                    server,
                    port,
                    generation,
                });
            }
            (false, Vec::new())
        }
        None => (false, Vec::new()),
    };
    let unchanged = if force {
        new.is_empty() && old.is_empty()
    } else {
        known && new == old
    };
    if !unchanged {
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
        let grants = new
            .iter()
            .map(|(server, port, generation)| LoopbackGrant {
                server: server.clone(),
                port: *port,
                generation: *generation,
                dial: dial(core, server, *generation, to.dir, *port),
            })
            .collect();
        net.grant(to.session, grants);
    }
    let given = Given {
        run: to.run,
        grants: new,
    };
    state.granted.insert(to.session.clone(), given);
}

/// Connects to `port` in the pod of `dir`, only while `server` is ready
/// with `generation` there.
pub(super) fn dial(
    core: &Core,
    server: &ServerId,
    generation: u64,
    dir: &Path,
    port: u16,
) -> PodDial {
    let weak = core.downgrade();
    let dev = core.dev_servers().dev.clone();
    let (server, dir) = (server.clone(), dir.to_path_buf());
    Arc::new(move || {
        let current = weak.upgrade().is_some_and(|core| {
            core.dev_servers().lock().servers.iter().any(|e| {
                e.id == server
                    && e.generation == generation
                    && e.status == ServerStatus::Ready
                    && e.place.dir == dir
                    && e.port == port
            })
        });
        let (dev, dir) = (dev.clone(), dir.clone());
        Box::pin(async move {
            if !current {
                return Err(ProtocolError::new(
                    ErrorCode::NotFound,
                    "this grant's server no longer runs that generation",
                ));
            }
            dev.connect(dir, port).await
        })
    })
}
