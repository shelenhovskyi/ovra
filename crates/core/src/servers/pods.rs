//! Pods: one sandbox per work dir, started with its first server and
//! ended with its last, each with a netproxy of its own (ADR 0008).

use super::{DevServerState, Place, Result, State};
use crate::Core;
use crate::protocol::agents::{SessionId, SessionMode};
use crate::protocol::netproxy::Allowlist;
use crate::protocol::projects::ProjectId;
use crate::protocol::servers::ServerDef;
use crate::services::{NetReporter, PodSpec, ProfileNetwork};

/// A running pod: its netproxy runs under a session id of its own.
#[derive(Debug)]
pub(super) struct Pod {
    project: ProjectId,
    session: SessionId,
    network: ProfileNetwork,
}

/// The pod of `place`, started with its netproxy if it does not run. Its
/// profile is an agent's for that work dir, exposing every defined port.
pub(super) async fn pod(core: &Core, place: &Place, defs: &[ServerDef]) -> Result<PodSpec> {
    let servers = core.dev_servers();
    let project = core.project_store().get(&place.project)?;
    let running = servers
        .lock()
        .pods
        .get(&place.dir)
        .map(|p| p.network.clone());
    let network = match running {
        Some(network) => network,
        None => {
            let (session, network) = start_proxy(core, &place.project).await?;
            let pod = Pod {
                project: place.project.clone(),
                session,
                network: network.clone(),
            };
            servers.lock().pods.insert(place.dir.clone(), pod);
            network
        }
    };
    let mode = match place.session {
        Some(_) => SessionMode::Isolated,
        None => SessionMode::Direct,
    };
    let mut profile = crate::agents::profile(&project, mode, &place.dir, network, Vec::new(), None);
    profile.expose = defs.iter().map(|d| d.port).collect();
    profile.kind = crate::services::ProfileKind::Pod;
    Ok(PodSpec {
        project: place.project.clone(),
        work_dir: place.dir.clone(),
        profile,
    })
}

/// Starts a pod's netproxy as `pod-<n>` with the project's allowlist and
/// no secrets; refusals are reported for that id.
async fn start_proxy(core: &Core, project: &ProjectId) -> Result<(SessionId, ProfileNetwork)> {
    let servers = core.dev_servers();
    let session = {
        let mut state = servers.lock();
        state.pods_made += 1;
        SessionId(format!("pod-{}", state.pods_made))
    };
    let Some(net) = &servers.net else {
        return Ok((session, ProfileNetwork::Open));
    };
    let store = core.project_store();
    let allow = store.allowlist(project)?;
    let reporter = NetReporter::new(project.clone(), session.clone(), core.sink())
        .with_core(Some(core.downgrade()));
    let started = net
        .start(session.clone(), allow.clone(), reporter, Vec::new())
        .await?;
    // The allowlist may have changed while it started.
    if let Ok(now) = store.allowlist(project)
        && now != allow
    {
        net.update(&session, now);
    }
    let network = ProfileNetwork::Proxy {
        address: started.address,
        ca_bundle: started.ca_bundle,
    };
    Ok((session, network))
}

/// Ends the pod's netproxy once none of its servers runs.
pub(super) fn end_pod_if_idle(servers: &DevServerState, state: &mut State, place: &Place) {
    if state
        .servers
        .iter()
        .any(|e| e.place.dir == place.dir && e.running())
    {
        return;
    }
    if let (Some(pod), Some(net)) = (state.pods.remove(&place.dir), &servers.net) {
        net.stop(&pod.session);
    }
}

/// Passes the project's new allowlist to its pods' netproxies.
pub(crate) fn allowlist_changed(core: &Core, project: &ProjectId, allow: &Allowlist) {
    let servers = core.dev_servers();
    let Some(net) = &servers.net else { return };
    for pod in servers.lock().pods.values() {
        if &pod.project == project {
            net.update(&pod.session, allow.clone());
        }
    }
}
