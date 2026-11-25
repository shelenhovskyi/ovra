//! Ovra's side of the pods: one sandboxed pod-init per work dir, driven
//! over its control socket, and the states and readiness of its servers.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use ovra_core::protocol::servers::{ServerId, ServerStatus};
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{
    CommandLine, DevServerSpec, PodConn, PodSpec, Result, Sandbox, SandboxStatus, ServerSink,
};
use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _, BufReader};
use tokio::net::UnixStream;
use tokio::process::Child;
use tokio::runtime::Handle;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::control::{
    CONTROL, Event, LISTENING, Lines, MAX_CONTROL_LINE, Request, Sig, Spawn, encode, port_socket,
};
use crate::init::POD_FLAG;
use crate::ready::{Next, Readiness};

/// How long one `ready.http` request may take.
const ASK_TIMEOUT: Duration = Duration::from_secs(2);

/// How long a pod whose control closed may take to end on its own.
const LET_END: Duration = Duration::from_millis(500);

pub(crate) struct Pods {
    sandbox: Arc<dyn Sandbox>,
    dir: PathBuf,
    pub(crate) program: PathBuf,
    state: Arc<Mutex<State>>,
}

#[derive(Default)]
struct State {
    pods: BTreeMap<PathBuf, Pod>,
    /// Names spawns in the control protocol.
    spawns: u64,
}

struct Pod {
    /// Its private folder; also tells this pod from a later one.
    dir: PathBuf,
    control: UnboundedSender<Request>,
    /// By spawn name.
    servers: BTreeMap<String, Running>,
}

struct Running {
    id: ServerId,
    generation: u64,
    port: u16,
    sink: Arc<dyn ServerSink>,
    ready: Readiness,
}

/// A report for a sink, made under the lock and sent after it.
struct Report(Arc<dyn ServerSink>, ServerId, u64, What);

enum What {
    Line(String),
    Status(ServerStatus),
}

impl Report {
    fn send(self) {
        let Self(sink, id, generation, what) = self;
        match what {
            What::Status(status) => sink.status(&id, generation, status),
            What::Line(line) => sink.output(&id, generation, vec![line]),
        }
    }
}

impl Running {
    fn status(&self, status: ServerStatus) -> Report {
        Report(
            self.sink.clone(),
            self.id.clone(),
            self.generation,
            What::Status(status),
        )
    }

    fn line(&self, line: String) -> Report {
        Report(
            self.sink.clone(),
            self.id.clone(),
            self.generation,
            What::Line(line),
        )
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn internal(message: impl Into<String>) -> ProtocolError {
    ProtocolError::new(ErrorCode::Internal, message)
}

impl std::fmt::Debug for Pods {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pods")
            .field("dir", &self.dir)
            .field("program", &self.program)
            .field("pods", &lock(&self.state).pods.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

impl Pods {
    pub(crate) fn new(sandbox: Arc<dyn Sandbox>, dir: PathBuf) -> Self {
        Self {
            sandbox,
            dir: std::path::absolute(&dir).unwrap_or(dir),
            program: std::env::current_exe().unwrap_or_default(),
            state: Arc::default(),
        }
    }

    pub(crate) fn status(&self) -> SandboxStatus {
        self.sandbox.status()
    }

    pub(crate) fn start(
        &self,
        pod: &PodSpec,
        server: DevServerSpec,
        sink: Arc<dyn ServerSink>,
    ) -> Result<()> {
        if let SandboxStatus::Unavailable { reason } = self.sandbox.status() {
            return Err(ProtocolError::new(
                ErrorCode::Unavailable,
                format!("servers run only in a sandbox: {reason}"),
            ));
        }
        let runtime = Handle::try_current().map_err(|e| internal(e.to_string()))?;
        let mut state = lock(&self.state);
        let pods = &state.pods;
        if pods
            .values()
            .any(|p| p.servers.values().any(|s| s.id == server.id))
        {
            let message = format!("server {} is running", server.id.0);
            return Err(ProtocolError::new(ErrorCode::Conflict, message));
        }
        let clash = pods.get(&pod.work_dir).map(|p| p.servers.values());
        if clash.is_some_and(|mut s| s.any(|s| s.port == server.port)) {
            let message = format!("port {} is taken in this pod", server.port);
            return Err(ProtocolError::new(ErrorCode::Conflict, message));
        }
        state.spawns += 1;
        let name = state.spawns.to_string();
        let spawn = Spawn::new(name.clone(), &server.line, server.port)
            .map_err(|e| ProtocolError::new(ErrorCode::BadRequest, e))?;
        let target = match state.pods.entry(pod.work_dir.clone()) {
            std::collections::btree_map::Entry::Occupied(e) => e.into_mut(),
            std::collections::btree_map::Entry::Vacant(e) => e.insert(self.open(&runtime, pod)?),
        };
        let _ = target.control.send(Request::Spawn(spawn));
        let running = Running {
            id: server.id,
            generation: server.generation,
            port: server.port,
            sink,
            ready: Readiness::new(server.ready.as_ref()),
        };
        target.servers.insert(name, running);
        Ok(())
    }

    /// A new pod for `spec.work_dir`, its pod-init started.
    fn open(&self, runtime: &Handle, spec: &PodSpec) -> Result<Pod> {
        let dir = new_folder(&self.dir)?;
        let child = self.launch(runtime, spec, &dir).inspect_err(|_| {
            let _ = std::fs::remove_dir_all(&dir);
        })?;
        let (control, requests) = unbounded_channel();
        let work_dir = spec.work_dir.clone();
        let state = self.state.clone();
        runtime.spawn(drive(state, work_dir, dir.clone(), child, requests));
        Ok(Pod {
            dir,
            control,
            servers: BTreeMap::new(),
        })
    }

    fn launch(&self, runtime: &Handle, spec: &PodSpec, dir: &Path) -> Result<Child> {
        let mut profile = spec.profile.clone();
        profile.write.push(dir.to_owned());
        let line = CommandLine {
            program: self.program.clone(),
            args: vec![POD_FLAG.into(), dir.into()],
            env: Vec::new(),
            cwd: spec.work_dir.clone(),
        };
        let line = self.sandbox.wrap(&profile, line)?;
        let mut command = tokio::process::Command::new(&line.program);
        command
            .args(&line.args)
            .current_dir(&line.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .kill_on_drop(true);
        for (name, value) in &line.env {
            match value {
                Some(value) => command.env(name, value),
                None => command.env_remove(name),
            };
        }
        let _entered = runtime.enter();
        command
            .spawn()
            .map_err(|e| internal(format!("starting the pod: {e}")))
    }

    pub(crate) fn stop(&self, id: &ServerId, grace: Duration) {
        if !signal(&self.state, id, Sig::Term) {
            return;
        }
        let Ok(runtime) = Handle::try_current() else {
            return;
        };
        let (state, id) = (self.state.clone(), id.clone());
        runtime.spawn(async move {
            tokio::time::sleep(grace).await;
            signal(&state, &id, Sig::Kill);
        });
    }

    pub(crate) async fn connect(&self, work_dir: PathBuf, port: u16) -> Result<PodConn> {
        let socket = {
            let state = lock(&self.state);
            let pod = state.pods.get(&work_dir);
            pod.filter(|p| p.servers.values().any(|s| s.port == port))
                .map(|p| p.dir.join(port_socket(port)))
        };
        let Some(socket) = socket else {
            let message = format!("no server on port {port} in this work dir");
            return Err(ProtocolError::new(ErrorCode::NotFound, message));
        };
        let conn = UnixStream::connect(&socket)
            .await
            .map_err(|e| internal(format!("port {port} of the pod: {e}")))?;
        Ok(Box::new(conn))
    }
}

impl Drop for Pods {
    fn drop(&mut self) {
        // Each pod's control closes: its pod-init kills its servers and
        // exits, or its driver kills it (`drive`).
        for (_, pod) in std::mem::take(&mut lock(&self.state).pods) {
            let _ = std::fs::remove_dir_all(&pod.dir);
        }
    }
}

/// Sends `sig` to the server's process group; false if it is not running.
fn signal(state: &Mutex<State>, id: &ServerId, sig: Sig) -> bool {
    let state = lock(state);
    for pod in state.pods.values() {
        if let Some((name, _)) = pod.servers.iter().find(|(_, s)| s.id == *id) {
            let _ = pod.control.send(Request::Signal {
                id: name.clone(),
                sig,
            });
            return true;
        }
    }
    false
}

/// A new folder in `dir` only this user can open, with a name nobody can
/// guess, for one pod's sockets.
fn new_folder(dir: &Path) -> Result<PathBuf> {
    let folder = dir.join(format!("{:016x}", rand::random::<u64>()));
    let mut builder = std::fs::DirBuilder::new();
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder
        .create(&folder)
        .map_err(|e| internal(format!("creating {}: {e}", folder.display())))?;
    Ok(folder)
}

/// Runs one pod until its last server ended or it failed, then kills it.
async fn drive(
    state: Arc<Mutex<State>>,
    work_dir: PathBuf,
    dir: PathBuf,
    mut child: Child,
    requests: UnboundedReceiver<Request>,
) {
    let reason = run(&state, &work_dir, &dir, &mut child, requests).await;
    let ended = {
        let mut state = lock(&state);
        match state.pods.get(&work_dir) {
            Some(pod) if pod.dir == dir => state.pods.remove(&work_dir),
            _ => None,
        }
    };
    let failed = ServerStatus::Failed { reason };
    for (_, server) in ended.map(|p| p.servers).unwrap_or_default() {
        server.status(failed.clone()).send();
    }
    // Its control is closed: pod-init ends its servers and exits.
    if tokio::time::timeout(LET_END, child.wait()).await.is_err() {
        let _ = child.kill().await;
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Returns why the pod ended, if it ended before its servers.
async fn run(
    state: &Arc<Mutex<State>>,
    work_dir: &Path,
    dir: &Path,
    child: &mut Child,
    mut requests: UnboundedReceiver<Request>,
) -> String {
    let Some(stdout) = child.stdout.take() else {
        return "the pod has no output".into();
    };
    let mut stdout = BufReader::new(stdout);
    loop {
        let mut line = String::new();
        match (&mut stdout).take(4096).read_line(&mut line).await {
            Ok(1..) if line.trim_end() == LISTENING => break,
            Ok(1..) => {}
            _ => return "the pod did not start".into(),
        }
    }
    let control = match UnixStream::connect(dir.join(CONTROL)).await {
        Ok(control) => control,
        Err(e) => return format!("the pod's control socket: {e}"),
    };
    let (mut read, mut write) = control.into_split();
    let mut lines = Lines::default();
    let mut buf = vec![0; 16 * 1024];
    loop {
        tokio::select! {
            request = requests.recv() => {
                let Some(request) = request else {
                    return "the pod was stopped".into();
                };
                if write.write_all(&encode(&request)).await.is_err() {
                    return "the pod ended".into();
                }
            }
            n = read.read(&mut buf) => {
                let n = n.unwrap_or(0);
                if n == 0 {
                    return "the pod ended".into();
                }
                for line in lines.push(&buf[..n], MAX_CONTROL_LINE) {
                    let Ok(event) = serde_json::from_slice(&line) else {
                        continue;
                    };
                    if handle(state, work_dir, dir, event) {
                        return String::new();
                    }
                }
            }
        }
    }
}

/// Applies one event; true once the pod's last server ended.
fn handle(state: &Arc<Mutex<State>>, work_dir: &Path, dir: &Path, event: Event) -> bool {
    let mut reports = Vec::new();
    let over = {
        let mut guard = lock(state);
        let Some(pod) = guard.pods.get_mut(work_dir).filter(|p| p.dir == dir) else {
            return true;
        };
        match event {
            Event::Output { id, line } => {
                if let Some(server) = pod.servers.get_mut(&id) {
                    let next = server.ready.line(&line);
                    reports.push(server.line(line));
                    if next == Next::Ready {
                        reports.push(server.status(ServerStatus::Ready));
                    }
                }
            }
            Event::Listening { id, port } => {
                if let Some(server) = pod.servers.get_mut(&id) {
                    match server.ready.listening() {
                        Next::Wait => {}
                        Next::Ready => reports.push(server.status(ServerStatus::Ready)),
                        Next::AskHttp(path) => {
                            let at = (work_dir.to_owned(), dir.to_owned());
                            tokio::spawn(ask_http(state.clone(), at, id, port, path));
                        }
                    }
                }
            }
            Event::Exited { id, code } => {
                if let Some(server) = pod.servers.remove(&id) {
                    reports.push(server.status(ServerStatus::Exited { code }));
                }
            }
            Event::Failed { id, reason } => {
                if let Some(server) = pod.servers.remove(&id) {
                    reports.push(server.status(ServerStatus::Failed { reason }));
                }
            }
        }
        let over = pod.servers.is_empty();
        if over {
            guard.pods.remove(work_dir);
        }
        over
    };
    reports.into_iter().for_each(Report::send);
    over
}

/// Asks `path` through the port's socket until it answers, then marks the
/// check passed. Gives up when the server ends.
async fn ask_http(
    state: Arc<Mutex<State>>,
    (work_dir, dir): (PathBuf, PathBuf),
    name: String,
    port: u16,
    path: String,
) {
    let socket = dir.join(port_socket(port));
    let mut wait = Duration::from_millis(50);
    let runs = || {
        let state = lock(&state);
        let pod = state.pods.get(&work_dir).filter(|p| p.dir == dir);
        pod.is_some_and(|p| p.servers.contains_key(&name))
    };
    while !answers(&socket, port, &path).await {
        tokio::time::sleep(wait).await;
        wait = (wait * 2).min(Duration::from_secs(1));
        if !runs() {
            return;
        }
    }
    let report = {
        let mut state = lock(&state);
        let pod = state.pods.get_mut(&work_dir).filter(|p| p.dir == dir);
        pod.and_then(|p| p.servers.get_mut(&name)).and_then(|s| {
            (s.ready.answered() == Next::Ready).then(|| s.status(ServerStatus::Ready))
        })
    };
    report.into_iter().for_each(Report::send);
}

/// Whether `GET <path>` answers with a status below 500.
async fn answers(socket: &Path, port: u16, path: &str) -> bool {
    let Some(head) = request_head(path, port) else {
        return false;
    };
    let ask = async {
        let mut conn = UnixStream::connect(socket).await?;
        conn.write_all(head.as_bytes()).await?;
        let mut status = [0; 12];
        conn.read_exact(&mut status).await?;
        Ok::<_, std::io::Error>(status)
    };
    match tokio::time::timeout(ASK_TIMEOUT, ask).await {
        Ok(Ok(status)) => answered(&status),
        _ => false,
    }
}

/// The request for `path`; `None` unless it is a plain absolute path.
fn request_head(path: &str, port: u16) -> Option<String> {
    let plain = path.starts_with('/') && path.bytes().all(|b| b.is_ascii_graphic());
    plain.then(|| {
        format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n")
    })
}

/// `HTTP/1.x NNN` with `NNN` below 500.
fn answered(status: &[u8; 12]) -> bool {
    let code = &status[9..];
    status.starts_with(b"HTTP/1.")
        && status[8] == b' '
        && code.iter().all(u8::is_ascii_digit)
        && code < &b"500"[..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_http_check_needs_a_plain_path_and_an_answer_below_500() {
        assert_eq!(
            request_head("/health?x=1", 5173).as_deref(),
            Some("GET /health?x=1 HTTP/1.1\r\nHost: 127.0.0.1:5173\r\nConnection: close\r\n\r\n")
        );
        assert_eq!(request_head("health", 5173), None);
        assert_eq!(request_head("/a b", 5173), None);
        assert_eq!(request_head("/a\r\nX: y", 5173), None);
        assert!(answered(b"HTTP/1.1 200"));
        assert!(answered(b"HTTP/1.0 404"));
        assert!(!answered(b"HTTP/1.1 502"));
        assert!(!answered(b"SSH-2.0-Open"));
    }
}
