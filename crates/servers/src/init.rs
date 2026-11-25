//! Pod-init: the first process in a server pod, run as `<ovra> --ovra-pod
//! <pod dir>` inside the sandbox. It listens on `<pod dir>/control.sock`
//! for Ovra (the first connection only), spawns servers and setup steps in
//! their own process groups, sends their output and ends, and relays
//! `<pod dir>/<port>.sock` to `127.0.0.1:<port>` inside the pod. When Ovra
//! goes away it kills every server and exits. On Linux its servers may not
//! make, remove or rename entries in `<pod dir>` (`confine`).

use std::collections::BTreeMap;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use rustix::process::{Pid, Signal, kill_process_group};
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::{TcpStream, UnixListener};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tokio::task::AbortHandle;

use crate::control::{
    CONTROL, Event, LISTENING, Lines, MAX_CONTROL_LINE, MAX_OUTPUT_LINE, Request, Sig, Spawn,
    encode, port_socket,
};

/// The first argument that puts the program in pod mode.
pub(crate) const POD_FLAG: &str = "--ovra-pod";

/// How long output may still arrive after a server's main process ended
/// (from children that left its process group).
const DRAIN: Duration = Duration::from_secs(1);

/// If this process was started as a pod, runs it and never returns.
/// Otherwise returns at once. Call it first in `main`, before any threads
/// start.
pub(crate) fn pod_init_if_asked() {
    let mut args = std::env::args_os().skip(1);
    if args.next().is_none_or(|flag| flag != POD_FLAG) {
        return;
    }
    let result = match (args.next(), args.next()) {
        (Some(dir), None) => tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .and_then(|rt| rt.block_on(serve(PathBuf::from(dir)))),
        _ => Err(std::io::Error::other("usage: --ovra-pod <pod dir>")),
    };
    if let Err(e) = result {
        eprintln!("ovra-pod: {e}");
        std::process::exit(1);
    }
    std::process::exit(0);
}

struct Server {
    group: Pid,
    /// `None` for a setup step.
    port: Option<u16>,
    tasks: Vec<AbortHandle>,
}

#[derive(Clone)]
struct Pod {
    dir: Arc<Path>,
    /// The servers' `TMPDIR`, when the one they would get holds the pod's
    /// folder ([`own_tmp`]).
    tmp: Option<Arc<Path>>,
    servers: Arc<Mutex<BTreeMap<String, Server>>>,
    events: UnboundedSender<Event>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

async fn serve(dir: PathBuf) -> std::io::Result<()> {
    let listener = UnixListener::bind(dir.join(CONTROL))?;
    println!("{LISTENING}");
    let (control, _) = listener.accept().await?;
    drop(listener);
    let _ = std::fs::remove_file(dir.join(CONTROL));
    let (mut read, mut write) = control.into_split();
    let (events, mut outbox) = unbounded_channel::<Event>();
    tokio::spawn(async move {
        while let Some(event) = outbox.recv().await {
            if write.write_all(&encode(&event)).await.is_err() {
                break;
            }
        }
    });
    let pod = Pod {
        tmp: own_tmp(&dir)?.map(Into::into),
        dir: dir.into(),
        servers: Arc::default(),
        events,
    };
    let mut lines = Lines::default();
    let mut buf = vec![0; 16 * 1024];
    loop {
        let n = read.read(&mut buf).await.unwrap_or(0);
        if n == 0 {
            break;
        }
        for line in lines.push(&buf[..n], MAX_CONTROL_LINE) {
            match serde_json::from_slice(&line) {
                Ok(Request::Spawn(spawn)) => pod.spawn(spawn),
                Ok(Request::Signal { id, sig }) => pod.signal(&id, sig),
                Err(e) => eprintln!("ovra-pod: bad request: {e}"),
            }
        }
    }
    // Ovra is gone: nothing in the pod may outlive it.
    for (_, server) in std::mem::take(&mut *lock(&pod.servers)) {
        let _ = kill_process_group(server.group, Signal::KILL);
    }
    if let Some(tmp) = &pod.tmp {
        let _ = std::fs::remove_dir_all(tmp);
    }
    Ok(())
}

/// A new folder for the servers' temporary files when the one they would
/// get (`TMPDIR`, else `/tmp`) holds the pod's folder: they may make no
/// entry in a folder on the way to it (`confine`), but below the others.
/// `None` when they can use their own.
fn own_tmp(dir: &Path) -> std::io::Result<Option<PathBuf>> {
    if !cfg!(target_os = "linux") {
        return Ok(None);
    }
    let tmp = std::env::var_os("TMPDIR")
        .map(PathBuf::from)
        .filter(|t| t.is_absolute())
        .unwrap_or_else(|| "/tmp".into());
    let (Ok(tmp), pod) = (std::fs::canonicalize(&tmp), std::fs::canonicalize(dir)?) else {
        return Ok(None);
    };
    if !pod.starts_with(&tmp) {
        return Ok(None);
    }
    let own = tmp.join(format!("ovra-pod-tmp-{:016x}", rand::random::<u64>()));
    let mut builder = std::fs::DirBuilder::new();
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(&own)?;
    Ok(Some(own))
}

impl Pod {
    fn send(&self, event: Event) {
        let _ = self.events.send(event);
    }

    fn signal(&self, id: &str, sig: Sig) {
        let signal = match sig {
            Sig::Term => Signal::TERM,
            Sig::Kill => Signal::KILL,
        };
        if let Some(server) = lock(&self.servers).get(id) {
            let _ = kill_process_group(server.group, signal);
        }
    }

    fn spawn(&self, spawn: Spawn) {
        let id = spawn.id.clone();
        if let Err(reason) = self.try_spawn(spawn) {
            self.send(Event::Failed { id, reason });
        }
    }

    fn try_spawn(&self, spawn: Spawn) -> Result<(), String> {
        let mut servers = lock(&self.servers);
        if servers.contains_key(&spawn.id) {
            return Err(format!("{} is running in this pod", spawn.id));
        }
        if let Some(port) = spawn.port
            && servers.values().any(|s| s.port == Some(port))
        {
            return Err(format!("port {port} is taken in this pod"));
        }
        let Some((program, args)) = spawn.argv.split_first() else {
            return Err("no program to run".into());
        };
        // A server's port is relayed from its socket; a step has neither.
        let socket = spawn.port.map(|port| self.dir.join(port_socket(port)));
        let relay = match &socket {
            Some(socket) => {
                let _ = std::fs::remove_file(socket);
                let relay = UnixListener::bind(socket)
                    .map_err(|e| format!("relay socket {}: {e}", socket.display()))?;
                Some(relay)
            }
            None => None,
        };
        let (output, writer) = tokio::net::unix::pipe::pipe()
            .and_then(|(w, r)| Ok((r, w.into_blocking_fd()?)))
            .map_err(|e| format!("output pipe: {e}"))?;
        let stderr = writer
            .try_clone()
            .map_err(|e| format!("output pipe: {e}"))?;
        let mut command = tokio::process::Command::new(program);
        command
            .args(args)
            .current_dir(&spawn.cwd)
            .stdin(Stdio::null())
            .stdout(writer)
            .stderr(stderr)
            .process_group(0);
        if let Some(tmp) = &self.tmp {
            command.env("TMPDIR", &**tmp);
        }
        for (name, value) in &spawn.env {
            match value {
                Some(value) => command.env(name, value),
                None => command.env_remove(name),
            };
        }
        #[cfg(target_os = "linux")]
        let child = crate::confine::spawn(&self.dir, &mut command);
        #[cfg(not(target_os = "linux"))]
        let child = command.spawn();
        drop(command);
        let mut child = child.map_err(|e| format!("could not start {program}: {e}"))?;
        let group = child
            .id()
            .and_then(|pid| Pid::from_raw(i32::try_from(pid).ok()?))
            .ok_or("the server has no process id")?;
        let port = spawn.port;
        let id = spawn.id;
        let tasks = match (relay, port) {
            (Some(relay), Some(port)) => vec![
                tokio::spawn(accept(relay, port)).abort_handle(),
                tokio::spawn(self.clone().probe(id.clone(), port)).abort_handle(),
            ],
            _ => Vec::new(),
        };
        servers.insert(id.clone(), Server { group, port, tasks });
        let pod = self.clone();
        tokio::spawn(async move {
            let lines = tokio::spawn(pod.clone().output(id.clone(), output));
            let status = child.wait().await;
            // The rest of its process group ends with it.
            let _ = kill_process_group(group, Signal::KILL);
            let _ = tokio::time::timeout(DRAIN, lines).await;
            if let Some(server) = lock(&pod.servers).remove(&id) {
                server.tasks.iter().for_each(AbortHandle::abort);
            }
            if let Some(socket) = &socket {
                let _ = std::fs::remove_file(socket);
            }
            let code = status.ok().and_then(|s| s.code());
            pod.send(Event::Exited { id, code });
        });
        Ok(())
    }

    async fn output(self, id: String, mut from: impl AsyncRead + Unpin) {
        let mut lines = Lines::default();
        let mut buf = vec![0; 16 * 1024];
        let send = |line: Vec<u8>| {
            let line = String::from_utf8_lossy(&line).into_owned();
            self.send(Event::Output {
                id: id.clone(),
                line,
            });
        };
        while let Ok(n @ 1..) = from.read(&mut buf).await {
            lines
                .push(&buf[..n], MAX_OUTPUT_LINE)
                .into_iter()
                .for_each(send);
        }
        lines.finish().into_iter().for_each(send);
    }

    /// Reports [`Event::Listening`] once the port accepts a connection.
    async fn probe(self, id: String, port: u16) {
        let mut wait = Duration::from_millis(10);
        while TcpStream::connect((Ipv4Addr::LOCALHOST, port))
            .await
            .is_err()
        {
            tokio::time::sleep(wait).await;
            wait = (wait * 2).min(Duration::from_millis(250));
        }
        self.send(Event::Listening { id, port });
    }
}

/// Relays each connection to `<port>.sock` to `127.0.0.1:<port>`.
async fn accept(relay: UnixListener, port: u16) {
    while let Ok((mut outside, _)) = relay.accept().await {
        tokio::spawn(async move {
            if let Ok(mut inside) = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).await {
                let _ = tokio::io::copy_bidirectional(&mut outside, &mut inside).await;
            }
        });
    }
}
