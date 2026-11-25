//! [`FakeDevServers`]: servers that run nothing; tests drive their reports.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::{lock, not_found};
use crate::protocol::servers::{ServerId, ServerStatus};
use crate::protocol::{ErrorCode, ProtocolError};
use crate::services::{
    BoxFuture, DevServerSpec, DevServers, PodConn, PodSpec, Result, SandboxStatus, ServerSink,
    SetupEnd, SetupSink, SetupStep,
};

/// Records each start and stop. A stop reports `Exited { code: None }`, as
/// a server ended by SIGTERM does; [`FakeDevServers::report`] sends any
/// other report. `connect` hands out the connections tests queued with
/// [`FakeDevServers::serve`] for a running server's work dir and port.
/// A pod ends with its last server, unless a test keeps it with
/// [`FakeDevServers::hold_pod`].
#[derive(Default)]
pub struct FakeDevServers {
    /// When set, `start` fails with `unavailable`.
    pub fails: bool,
    pub running: Mutex<BTreeMap<ServerId, FakeDevServer>>,
    /// Every `stop` call, in order.
    pub stopped: Mutex<Vec<(ServerId, Duration)>>,
    /// Every `setup` call: the pod and its steps.
    pub setups: Mutex<Vec<(PodSpec, Vec<SetupStep>)>>,
    /// How the next `setup` ends; `None` is [`SetupEnd::Done`]. Each step
    /// up to the one it names is reported as started.
    pub setup_end: Mutex<Option<SetupEnd>>,
    queued: Mutex<BTreeMap<(PathBuf, u16), Vec<PodConn>>>,
    held: Mutex<BTreeSet<PathBuf>>,
    /// Bumped whenever a server or a held pod ends.
    changed: tokio::sync::watch::Sender<u64>,
}

/// One running fake server.
#[derive(Debug, Clone)]
pub struct FakeDevServer {
    pub pod: PodSpec,
    pub spec: DevServerSpec,
    pub sink: Arc<dyn ServerSink>,
}

impl std::fmt::Debug for FakeDevServers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeDevServers")
            .field("fails", &self.fails)
            .field("running", &self.running)
            .field("stopped", &self.stopped)
            .field("setups", &self.setups)
            .field("queued", &lock(&self.queued).len())
            .finish()
    }
}

impl FakeDevServers {
    /// Queues a connection for the next `connect` to `port` in `work_dir`.
    pub fn serve(&self, work_dir: impl Into<PathBuf>, port: u16, conn: PodConn) {
        lock(&self.queued)
            .entry((work_dir.into(), port))
            .or_default()
            .push(conn);
    }

    /// Keeps the pod of `work_dir` running after its last server, as a
    /// process that is slow to exit would, until [`Self::release_pod`].
    pub fn hold_pod(&self, work_dir: impl Into<PathBuf>) {
        lock(&self.held).insert(work_dir.into());
    }

    pub fn release_pod(&self, work_dir: &std::path::Path) {
        lock(&self.held).remove(work_dir);
        self.changed.send_modify(|n| *n += 1);
    }

    /// Reports `status` for the running server's current generation; an
    /// ended status removes it. Returns whether it was running.
    pub fn report(&self, id: &ServerId, status: ServerStatus) -> bool {
        let ended = matches!(
            status,
            ServerStatus::Exited { .. } | ServerStatus::Failed { .. }
        );
        let server = {
            let mut running = lock(&self.running);
            if ended {
                running.remove(id)
            } else {
                running.get(id).cloned()
            }
        };
        let Some(server) = server else {
            return false;
        };
        server.sink.status(id, server.spec.generation, status);
        if ended {
            self.changed.send_modify(|n| *n += 1);
        }
        true
    }

    /// Reports output lines for the running server's current generation.
    pub fn output(&self, id: &ServerId, lines: &[&str]) -> bool {
        let Some(server) = lock(&self.running).get(id).cloned() else {
            return false;
        };
        let lines = lines.iter().map(|l| (*l).to_owned()).collect();
        server.sink.output(id, server.spec.generation, lines);
        true
    }
}

impl DevServers for FakeDevServers {
    fn status(&self) -> SandboxStatus {
        SandboxStatus::Available {
            name: "fake".into(),
            gaps: Vec::new(),
        }
    }

    fn start(&self, pod: &PodSpec, server: DevServerSpec, sink: Arc<dyn ServerSink>) -> Result<()> {
        if self.fails {
            return Err(ProtocolError::new(
                ErrorCode::Unavailable,
                "the fake servers fail",
            ));
        }
        let mut running = lock(&self.running);
        let clash = running
            .values()
            .any(|s| s.pod.work_dir == pod.work_dir && s.spec.port == server.port);
        if clash || running.contains_key(&server.id) {
            return Err(ProtocolError::new(
                ErrorCode::Conflict,
                format!("port {} is taken in this pod", server.port),
            ));
        }
        let fake = FakeDevServer {
            pod: pod.clone(),
            spec: server,
            sink,
        };
        running.insert(fake.spec.id.clone(), fake);
        Ok(())
    }

    fn stop(&self, id: &ServerId, grace: Duration) {
        lock(&self.stopped).push((id.clone(), grace));
        self.report(id, ServerStatus::Exited { code: None });
    }

    fn connect(&self, work_dir: PathBuf, port: u16) -> BoxFuture<'_, Result<PodConn>> {
        Box::pin(async move {
            let listens = lock(&self.running)
                .values()
                .any(|s| s.pod.work_dir == work_dir && s.spec.port == port);
            let conn = listens
                .then(|| lock(&self.queued).get_mut(&(work_dir.clone(), port))?.pop())
                .flatten();
            conn.ok_or_else(|| not_found(format!("a server on port {port}")))
        })
    }

    fn setup(
        &self,
        pod: &PodSpec,
        steps: Vec<SetupStep>,
        sink: Arc<dyn SetupSink>,
    ) -> BoxFuture<'_, Result<SetupEnd>> {
        let end = lock(&self.setup_end).take().unwrap_or(SetupEnd::Done);
        let last = match end {
            SetupEnd::Done => steps.len(),
            SetupEnd::Failed { step, .. } | SetupEnd::TimedOut { step } => step + 1,
        };
        for index in 0..last.min(steps.len()) {
            sink.step(index);
        }
        lock(&self.setups).push((pod.clone(), steps));
        Box::pin(async move { Ok(end) })
    }

    fn pod_ended(&self, work_dir: PathBuf) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            let mut changed = self.changed.subscribe();
            loop {
                let runs = lock(&self.held).contains(&work_dir)
                    || lock(&self.running)
                        .values()
                        .any(|s| s.pod.work_dir == work_dir);
                if !runs || changed.changed().await.is_err() {
                    return;
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::projects::ProjectId;
    use crate::services::{CommandLine, ProfileNetwork, SandboxProfile};

    #[derive(Debug, Default)]
    struct Recorder(Mutex<Vec<(u64, String)>>);

    impl ServerSink for Recorder {
        fn output(&self, _: &ServerId, generation: u64, lines: Vec<String>) {
            lock(&self.0).push((generation, lines.join("|")));
        }
        fn status(&self, _: &ServerId, generation: u64, status: ServerStatus) {
            lock(&self.0).push((generation, format!("{status:?}")));
        }
    }

    fn pod(work_dir: &str) -> PodSpec {
        PodSpec {
            project: ProjectId("p1".into()),
            work_dir: work_dir.into(),
            profile: SandboxProfile {
                kind: crate::services::ProfileKind::Pod,
                claude_dir: None,
                work_dir: work_dir.into(),
                read: Vec::new(),
                write: Vec::new(),
                expose: vec![5173],
                forward: Vec::new(),
                network: ProfileNetwork::Blocked,
            },
        }
    }

    fn spec(id: &str, generation: u64) -> DevServerSpec {
        DevServerSpec {
            id: ServerId(id.into()),
            generation,
            line: CommandLine {
                program: "pnpm".into(),
                args: vec!["dev".into()],
                env: Vec::new(),
                cwd: "/w".into(),
            },
            port: 5173,
            ready: None,
        }
    }

    #[tokio::test]
    async fn reports_carry_the_generation_and_a_stop_ends_the_server() {
        let servers = FakeDevServers::default();
        let sink = Arc::new(Recorder::default());
        let id = ServerId("srv1".into());
        servers
            .start(&pod("/w"), spec("srv1", 3), sink.clone())
            .expect("start");
        // Another server on the same port of the same pod clashes; another
        // pod has its own loopback.
        assert!(
            servers
                .start(&pod("/w"), spec("srv2", 1), sink.clone())
                .is_err()
        );
        servers
            .start(&pod("/wt"), spec("srv3", 1), sink.clone())
            .expect("other pod");
        assert!(servers.output(&id, &["ready in 300 ms"]));
        assert!(servers.report(&id, ServerStatus::Ready));
        servers.stop(&id, Duration::from_secs(5));
        assert!(!servers.report(&id, ServerStatus::Ready));
        assert_eq!(
            *lock(&sink.0),
            [
                (3, "ready in 300 ms".to_owned()),
                (3, "Ready".to_owned()),
                (3, "Exited { code: None }".to_owned()),
            ]
        );
    }

    #[derive(Debug, Default)]
    struct Steps(Mutex<Vec<usize>>);

    impl SetupSink for Steps {
        fn step(&self, index: usize) {
            lock(&self.0).push(index);
        }
        fn output(&self, _: usize, _: Vec<String>) {}
    }

    #[tokio::test]
    async fn setup_reports_the_steps_up_to_the_one_that_failed() {
        let servers = FakeDevServers::default();
        let step = SetupStep {
            line: spec("x", 1).line,
            timeout: Duration::from_secs(600),
        };
        let steps = vec![step.clone(), step.clone(), step];
        let sink = Arc::new(Steps::default());
        let done = servers
            .setup(&pod("/wt"), steps.clone(), sink.clone())
            .await;
        assert_eq!(done.ok(), Some(SetupEnd::Done));
        assert_eq!(*lock(&sink.0), [0, 1, 2]);

        let failed = SetupEnd::Failed {
            step: 1,
            code: Some(1),
            reason: None,
        };
        *lock(&servers.setup_end) = Some(failed.clone());
        let sink = Arc::new(Steps::default());
        let end = servers.setup(&pod("/wt"), steps, sink.clone()).await;
        assert_eq!(end.ok(), Some(failed));
        assert_eq!(*lock(&sink.0), [0, 1]);
        assert_eq!(lock(&servers.setups).len(), 2);
    }

    #[tokio::test]
    async fn connect_reaches_only_a_running_server_of_that_pod() {
        let servers = FakeDevServers::default();
        servers.serve("/w", 5173, Box::new(tokio::io::duplex(64).0));
        assert!(servers.connect("/w".into(), 5173).await.is_err());
        servers
            .start(&pod("/w"), spec("srv1", 1), Arc::new(Recorder::default()))
            .expect("start");
        assert!(servers.connect("/wt".into(), 5173).await.is_err());
        assert!(servers.connect("/w".into(), 5173).await.is_ok());
        // Each queued connection is handed out once.
        assert!(servers.connect("/w".into(), 5173).await.is_err());
    }
}
