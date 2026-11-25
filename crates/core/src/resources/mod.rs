//! How much memory and CPU agent sessions take, and soft balancing
//! (`docs/specs/agent-resources.md`). While any session has not ended, the
//! core samples every [`INTERVAL`] and sends `resources.sample` per project.
//! While memory is low or the CPU stays busy, queued workers wait; running
//! agents are never stopped.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use crate::Core;
use crate::agents::Running as RunningSession;
use crate::core::WeakCore;
use crate::protocol::agents::SessionMode;
use crate::protocol::projects::ProjectId;
use crate::protocol::resources::{Pressure, ResourcesEvent, SessionResources, SystemLoad, Usage};
use crate::services::ResourceMonitor;

/// Time between samples.
pub(crate) const INTERVAL: Duration = Duration::from_secs(2);
/// Samples the CPU mean covers: 30 s.
const CPU_WINDOW: usize = 15;
const GIB: u64 = 1 << 30;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The monitor, if one is wired, and what the samples so far say.
#[derive(Debug)]
pub(crate) struct Resources {
    monitor: Option<Arc<dyn ResourceMonitor>>,
    state: Mutex<State>,
    /// Takes the next sample now instead of at the next interval.
    now: Arc<tokio::sync::Notify>,
}

#[derive(Debug, Default)]
struct State {
    /// A sampling loop runs.
    running: bool,
    /// The machine's CPU share of the last samples, oldest first.
    cpu: VecDeque<u32>,
    pressure: Option<Pressure>,
    /// A pressure ended and queued workers start one per sample until none
    /// waits for anything but a place, so they do not all start at once and
    /// bring it back.
    ramp: bool,
    /// Projects the last sample was sent for.
    sampled: BTreeSet<ProjectId>,
}

impl Resources {
    pub(crate) fn new(monitor: Option<Arc<dyn ResourceMonitor>>) -> Self {
        Self {
            monitor,
            state: Mutex::default(),
            now: Arc::default(),
        }
    }
}

/// Whether queued workers must wait although a place may be free: memory
/// is low, the CPU is busy, or one just stopped being so and they start
/// one per sample.
pub(crate) fn holds_back(core: &Core) -> bool {
    let state = lock(&core.resources().state);
    state.pressure.is_some() || state.ramp
}

/// Whether a sampling loop runs.
#[cfg(test)]
pub(crate) fn sampling(core: &Core) -> bool {
    lock(&core.resources().state).running
}

/// A session started or changed state: samples while any has not ended.
pub(crate) fn wake(core: &Core) {
    let resources = core.resources();
    if resources.monitor.is_none() {
        return;
    }
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        return;
    };
    {
        let mut state = lock(&resources.state);
        if state.running {
            return;
        }
        state.running = true;
    }
    runtime.spawn(run(core.downgrade(), resources.now.clone()));
}

/// An agent process started: it shows with the next sample, taken now.
pub(crate) fn sample_now(core: &Core) {
    core.resources().now.notify_one();
}

async fn run(core: WeakCore, now: Arc<tokio::sync::Notify>) {
    let mut next = tokio::time::Instant::now();
    loop {
        // Woken early by `sample_now`; at once the first time.
        let ticked = tokio::time::timeout_at(next, now.notified()).await.is_err();
        if ticked {
            next = tokio::time::Instant::now() + INTERVAL;
        }
        let Some(core) = core.upgrade() else {
            return;
        };
        if !step(&core, ticked).await {
            return;
        }
    }
}

/// One sample; `false` once no session runs and the loop has stopped.
/// Only samples at the interval (`ticked`) count for the CPU's mean and
/// start workers after a shortage, so early ones change neither pace.
async fn step(core: &Core, ticked: bool) -> bool {
    let resources = core.resources();
    let Some(monitor) = resources.monitor.clone() else {
        return false;
    };
    let sessions = {
        // Checked under the lock `wake` takes, so a session that starts
        // now either sees the loop running or starts a new one.
        let mut state = lock(&resources.state);
        let sessions = core.sessions().not_ended();
        if sessions.is_empty() {
            let gone = std::mem::take(&mut *state).sampled;
            drop(state);
            // Their lines and any pressure note go with the last session.
            for project in gone {
                emit(core, project, SystemLoad::default(), None, Vec::new());
            }
            return false;
        }
        sessions
    };
    let roots = Roots::of(core, &sessions);
    let pids = roots.pids.clone();
    let Ok(sample) = tokio::task::spawn_blocking(move || monitor.sample(&pids)).await else {
        return true;
    };
    let by_project = roots.by_project(&sample.trees);
    let (pressure, ramp, gone) = {
        let mut state = lock(&resources.state);
        if ticked {
            state.cpu.push_back(sample.system.cpu_percent);
        }
        while state.cpu.len() > CPU_WINDOW {
            state.cpu.pop_front();
        }
        let before = state.pressure;
        state.pressure = judge(before, &sample.system, &state.cpu);
        if before.is_some() && state.pressure.is_none() {
            state.ramp = true;
        }
        let now: BTreeSet<ProjectId> = by_project.keys().cloned().collect();
        let gone: Vec<ProjectId> = state.sampled.difference(&now).cloned().collect();
        state.sampled = now;
        (state.pressure, state.ramp, gone)
    };
    for project in gone {
        emit(core, project, sample.system, pressure, Vec::new());
    }
    for (project, list) in by_project {
        emit(core, project, sample.system, pressure, list);
    }
    if ticked && pressure.is_none() && ramp {
        // Read again: a worker may have been queued while this sampled.
        let queued: BTreeSet<ProjectId> = core
            .sessions()
            .not_ended()
            .into_iter()
            .filter(|s| s.queued)
            .map(|s| s.project)
            .collect();
        let mut started = false;
        for project in queued {
            started |= crate::orchestration::start_next_queued(core, &project);
        }
        // Nothing waits, or only for a place: the shortage is over.
        if !started {
            lock(&resources.state).ramp = false;
        }
    }
    true
}

fn emit(
    core: &Core,
    project: ProjectId,
    system: SystemLoad,
    pressure: Option<Pressure>,
    sessions: Vec<SessionResources>,
) {
    core.emit(ResourcesEvent::Sample {
        project,
        system,
        pressure,
        sessions,
    });
}

/// Whether the machine is short of memory or CPU now, given what it was.
/// Each pressure starts at one level and ends at a kinder one, so it does
/// not flap.
fn judge(before: Option<Pressure>, system: &SystemLoad, cpu: &VecDeque<u32>) -> Option<Pressure> {
    let total = system.memory_total_bytes;
    let available = system.memory_available_bytes;
    let low = if before == Some(Pressure::LowMemory) {
        available < (GIB + GIB / 2).max(total / 100 * 15)
    } else {
        available < GIB.max(total / 10)
    };
    if total > 0 && low {
        return Some(Pressure::LowMemory);
    }
    let samples = u64::try_from(cpu.len()).unwrap_or(u64::MAX).max(1);
    let mean = cpu.iter().map(|&c| u64::from(c)).sum::<u64>() / samples;
    let busy = if before == Some(Pressure::BusyCpu) {
        mean >= 75
    } else {
        cpu.len() >= CPU_WINDOW && mean >= 90
    };
    busy.then_some(Pressure::BusyCpu)
}

/// The process trees to sample: per session, its agent, the pod of its
/// worktree and its browser, each where one runs.
struct Roots {
    pids: Vec<u32>,
    /// Per session, where its parts are in `pids`.
    sessions: Vec<(ProjectId, SessionParts)>,
}

struct SessionParts {
    session: crate::protocol::agents::SessionId,
    agent: Option<usize>,
    servers: Option<usize>,
    browser: Option<usize>,
}

impl Roots {
    fn of(core: &Core, sessions: &[RunningSession]) -> Self {
        let mut pids = Vec::new();
        let mut at = |pid: Option<u32>| {
            pid.map(|pid| {
                pids.push(pid);
                pids.len() - 1
            })
        };
        let mut parts = Vec::new();
        for s in sessions {
            // Pods of the main checkout are the person's, not the direct
            // session's.
            let pod = match (&s.cwd, s.mode) {
                (Some(cwd), SessionMode::Isolated) => core.dev_servers().pod_pid(cwd),
                _ => None,
            };
            let part = SessionParts {
                session: s.id.clone(),
                agent: at(s.pid),
                servers: at(pod),
                browser: at(core.browser_state().pid(&s.id)),
            };
            parts.push((s.project.clone(), part));
        }
        Self {
            pids,
            sessions: parts,
        }
    }

    fn by_project(&self, trees: &[Option<Usage>]) -> BTreeMap<ProjectId, Vec<SessionResources>> {
        let usage = |at: Option<usize>| at.and_then(|i| trees.get(i).copied().flatten());
        let mut out: BTreeMap<ProjectId, Vec<SessionResources>> = BTreeMap::new();
        for (project, part) in &self.sessions {
            out.entry(project.clone())
                .or_default()
                .push(SessionResources {
                    session: part.session.clone(),
                    agent: usage(part.agent),
                    servers: usage(part.servers),
                    browser: usage(part.browser),
                });
        }
        out
    }
}

#[cfg(test)]
mod tests;
