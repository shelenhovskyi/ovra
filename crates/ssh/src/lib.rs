//! Remote machines over the person's own OpenSSH (ADR 0016,
//! `docs/specs/ssh-remotes.md`). Security code: every change here gets an
//! independent security review.
//!
//! [`OpenSsh`] keeps the person's remote machines and pins their host
//! keys under Ovra's data folder (`ssh/hosts.json`, `ssh/<id>/known_hosts`).
//! [`OpenSsh::connect`] opens a [`Link`] to one of them.

mod config;
pub mod install;
mod known;
mod link;
mod tools;
pub mod window;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use ovra_core::protocol::ssh::{PreparedHost, RemoteHost};
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{BoxFuture, RemoteHosts, Result as ServiceResult};
use ovra_core::ssh::{DaemonPath, Destination, HostId, MachineName, fingerprints_ok};
use rand::Rng;

#[cfg(feature = "fake")]
pub use crate::link::fake;
pub use crate::link::{Failure, Link, Reason, Sender};

use crate::config::Effective;
use crate::known::Known;
use crate::tools::Tools;

/// Remote machines over the person's own OpenSSH.
#[derive(Debug)]
pub struct OpenSsh {
    /// `<data>/ssh`.
    dir: PathBuf,
    /// Held while the host list changes.
    changing: Mutex<()>,
    /// `ssh` and `ssh-keygen`, once found.
    tools: Mutex<Option<Tools>>,
    /// An SSH config other than the person's, for tests.
    config: Option<PathBuf>,
}

impl OpenSsh {
    /// Keeps hosts and pins under `data_dir/ssh`.
    pub fn new(data_dir: &Path) -> Self {
        Self {
            dir: data_dir.join("ssh"),
            changing: Mutex::new(()),
            tools: Mutex::new(None),
            config: None,
        }
    }

    /// Reads `config` instead of the person's `~/.ssh/config`.
    #[cfg(test)]
    fn with_config(data_dir: &Path, config: PathBuf) -> Self {
        Self {
            config: Some(config),
            ..Self::new(data_dir)
        }
    }

    /// The tools found on `PATH` the first time they were found.
    async fn tools(&self) -> ServiceResult<Tools> {
        if let Some(tools) = lock(&self.tools).clone() {
            return Ok(tools);
        }
        let tools = Tools::find(std::env::var_os("PATH")).await?;
        *lock(&self.tools) = Some(tools.clone());
        Ok(tools)
    }

    /// `ssh -G` for `destination` as the person's config resolves it.
    async fn resolve(&self, tools: &Tools, destination: &Destination) -> ServiceResult<Effective> {
        let mut command = tools.ssh();
        if let Some(config) = &self.config {
            command.arg("-F").arg(config);
        }
        command.args(["-G", "--", destination.as_str()]);
        let out = tools.run(command, None).await?;
        if !out.status.success() {
            return Err(ProtocolError::new(
                ErrorCode::BadRequest,
                format!(
                    "your SSH config could not be read for {}",
                    destination.as_str()
                ),
            ));
        }
        Ok(Effective::parse(&String::from_utf8_lossy(&out.stdout)))
    }

    async fn found(
        &self,
        destination: &Destination,
    ) -> ServiceResult<(Effective, String, Vec<Known>)> {
        let tools = self.tools().await?;
        let effective = self.resolve(&tools, destination).await?;
        // Ovra cannot read a revocation list from here (`ssh-keygen -Q`
        // takes key files only), so it shows no key rather than one the
        // person revoked.
        if effective
            .get("revokedhostkeys")
            .is_some_and(|v| v != "none")
        {
            return Err(ProtocolError::new(
                ErrorCode::Unavailable,
                "your SSH config revokes host keys (RevokedHostKeys), which Ovra cannot check yet",
            ));
        }
        let name = effective.key_name().ok_or_else(|| {
            ProtocolError::new(
                ErrorCode::BadRequest,
                "your SSH config names no host for this",
            )
        })?;
        let keys = known::lookup(&tools, &name, &effective.known_files()).await?;
        Ok((effective, name, keys))
    }

    fn hosts_file(&self) -> PathBuf {
        self.dir.join("hosts.json")
    }

    /// The saved hosts, each checked again as if it had just been added: a
    /// command line is only ever built from checked values.
    fn read(&self) -> ServiceResult<Vec<RemoteHost>> {
        let hosts: Vec<RemoteHost> = match std::fs::read(self.hosts_file()) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| internal(&e))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(internal(&e)),
        };
        for host in &hosts {
            let checked = HostId::parse(&host.id)
                .and(MachineName::parse(&host.name))
                .and(Destination::parse(&host.destination))
                .and(DaemonPath::parse(Some(&host.daemon)))
                .is_ok()
                && fingerprints_ok(&host.fingerprints);
            if !checked {
                return Err(internal(&format!(
                    "{} was changed outside Ovra; fix or delete it",
                    self.hosts_file().display()
                )));
            }
        }
        Ok(hosts)
    }

    fn write(&self, hosts: &[RemoteHost]) -> ServiceResult<()> {
        private_dir(&self.dir)?;
        let bytes = serde_json::to_vec_pretty(hosts).map_err(|e| internal(&e))?;
        write_private(&self.hosts_file(), &bytes)
    }

    /// Writes the pins for `pinned` under a new id and the host to the list.
    fn save(
        &self,
        name: &MachineName,
        destination: &Destination,
        daemon: &DaemonPath,
        pinned: &[&Known],
    ) -> ServiceResult<RemoteHost> {
        let _changing = lock(&self.changing);
        let mut hosts = self.read()?;
        let id = new_id();
        let folder = self.dir.join(id.as_str());
        private_dir(&self.dir)?;
        private_dir(&folder)?;
        let lines: String = pinned
            .iter()
            .map(|k| format!("ovra-{} {} {}\n", id.as_str(), k.key.kind, k.blob))
            .collect();
        let host = RemoteHost {
            id: id.as_str().to_owned(),
            name: name.as_str().to_owned(),
            destination: destination.as_str().to_owned(),
            daemon: daemon.as_str().to_owned(),
            fingerprints: pinned.iter().map(|k| k.key.fingerprint.clone()).collect(),
            last_connected: None,
        };
        hosts.push(host.clone());
        let saved = write_private(&folder.join("known_hosts"), lines.as_bytes())
            .and_then(|()| self.write(&hosts));
        if saved.is_err() {
            let _ = std::fs::remove_dir_all(&folder);
        }
        saved.map(|()| host)
    }
}

impl OpenSsh {
    /// Points the record of `id` at `daemon`.
    fn set_daemon(&self, id: &HostId, daemon: &DaemonPath) -> ServiceResult<RemoteHost> {
        let _changing = lock(&self.changing);
        let mut hosts = self.read()?;
        let host = hosts
            .iter_mut()
            .find(|h| h.id == id.as_str())
            .ok_or_else(|| missing(id))?;
        daemon.as_str().clone_into(&mut host.daemon);
        let host = host.clone();
        self.write(&hosts)?;
        Ok(host)
    }

    /// Connects to the machine `id` with the hardened command line, after
    /// `ssh -G` showed that it won over the person's config.
    pub async fn connect(&self, id: &HostId) -> Result<Link, Failure> {
        self.connect_with(id, false).await
    }

    /// [`OpenSsh::connect`] that first asks a core of another build,
    /// still running there, to quit (`version_mismatch`). Its agents stop.
    pub async fn restart(&self, id: &HostId) -> Result<Link, Failure> {
        self.connect_with(id, true).await
    }

    async fn connect_with(&self, id: &HostId, restart: bool) -> Result<Link, Failure> {
        let what = if restart { What::Restart } else { What::Attach };
        let mut command = self.command(id, what).await?;
        link::pipes(&mut command);
        let child = command.spawn().map_err(|e| self.lost_ssh(&e))?;
        link::open(tools::Group(child)).await
    }

    /// `ssh` has moved or was upgraded since it was found: look again next
    /// time.
    fn lost_ssh(&self, e: &std::io::Error) -> Failure {
        *lock(&self.tools) = None;
        Failure::new(Reason::NoSsh, format!("could not run ssh: {e}"))
    }

    /// `ssh` to the machine `id` with the hardened command line running
    /// `what`, after `ssh -G` showed that it won over the person's config;
    /// in its own process group, with the kept variables only.
    async fn command(
        &self,
        id: &HostId,
        what: What<'_>,
    ) -> Result<tokio::process::Command, Failure> {
        let host = self
            .read()
            .map_err(|e| Failure::new(Reason::NoPin, e.message))?
            .into_iter()
            .find(|h| h.id == id.as_str())
            .ok_or_else(|| Failure::new(Reason::NoPin, missing(id).message))?;
        let pins = self.dir.join(id.as_str()).join("known_hosts");
        if !pins.is_file() {
            return Err(Failure::new(
                Reason::NoPin,
                "the pinned keys of this machine are gone; remove it and add it again",
            ));
        }
        let unsafe_config = |e: ProtocolError| Failure::new(Reason::UnsafeConfig, e.message);
        let destination = Destination::parse(&host.destination).map_err(unsafe_config)?;
        let daemon = DaemonPath::parse(Some(&host.daemon)).map_err(unsafe_config)?;
        let remote = match what {
            What::Attach => Remote::Attach(&daemon),
            What::Restart => Remote::Restart(&daemon),
            What::Uname => Remote::Uname,
            What::Install(sha) => Remote::Install(sha),
        };
        let args = arguments(&pins, id, &destination, remote).map_err(unsafe_config)?;
        let tools = self
            .tools()
            .await
            .map_err(|e| Failure::new(Reason::NoSsh, e.message))?;

        // The same options, without the remote command.
        let mut check = tools.ssh();
        if let Some(config) = &self.config {
            check.arg("-F").arg(config);
        }
        check.arg("-G").args(options(&args));
        let out = tools.run(check, None).await.map_err(unsafe_config)?;
        if !out.status.success() {
            return Err(Failure::new(
                Reason::UnsafeConfig,
                "your SSH config could not be read for this machine",
            ));
        }
        let pins_text = pins.to_str().unwrap_or_default();
        Effective::parse(&String::from_utf8_lossy(&out.stdout))
            .check(pins_text, &format!("ovra-{}", id.as_str()))
            .map_err(unsafe_config)?;

        let mut command = tools.ssh();
        if let Some(config) = &self.config {
            command.arg("-F").arg(config);
        }
        command.args(&args);
        tools::own_group(&mut command);
        Ok(command)
    }
}

/// What a command line from [`OpenSsh::command`] runs on the machine.
#[derive(Clone, Copy)]
enum What<'a> {
    Attach,
    Restart,
    Uname,
    Install(&'a install::Sha256),
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl RemoteHosts for OpenSsh {
    fn list(&self) -> ServiceResult<Vec<RemoteHost>> {
        self.read()
    }

    fn prepare<'a>(
        &'a self,
        destination: &'a Destination,
    ) -> BoxFuture<'a, ServiceResult<PreparedHost>> {
        Box::pin(async move {
            let (effective, key_name, keys) = self.found(destination).await?;
            Ok(PreparedHost {
                destination: destination.as_str().to_owned(),
                host_name: effective.get("hostname").unwrap_or_default().to_owned(),
                port: effective
                    .get("port")
                    .and_then(|p| p.parse().ok())
                    .unwrap_or(22),
                user: effective.get("user").unwrap_or_default().to_owned(),
                key_name,
                keys: keys.into_iter().map(|k| k.key).collect(),
            })
        })
    }

    fn add<'a>(
        &'a self,
        name: &'a MachineName,
        destination: &'a Destination,
        daemon: &'a DaemonPath,
        fingerprints: &'a [String],
    ) -> BoxFuture<'a, ServiceResult<RemoteHost>> {
        Box::pin(async move {
            let (_, _, keys) = self.found(destination).await?;
            let pinned: Vec<&Known> = fingerprints
                .iter()
                .filter_map(|f| keys.iter().find(|k| &k.key.fingerprint == f))
                .collect();
            if pinned.len() != fingerprints.len() {
                return Err(ProtocolError::new(
                    ErrorCode::Conflict,
                    "your SSH no longer trusts these keys for this machine; check them again",
                ));
            }
            self.save(name, destination, daemon, &pinned)
        })
    }

    fn remove(&self, id: &HostId) -> ServiceResult<()> {
        let _changing = lock(&self.changing);
        let mut hosts = self.read()?;
        let before = hosts.len();
        hosts.retain(|h| h.id != id.as_str());
        if hosts.len() == before {
            return Err(missing(id));
        }
        self.write(&hosts)?;
        match std::fs::remove_dir_all(self.dir.join(id.as_str())) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(internal(&e)),
            _ => Ok(()),
        }
    }

    fn rename(&self, id: &HostId, name: &MachineName) -> ServiceResult<RemoteHost> {
        let _changing = lock(&self.changing);
        let mut hosts = self.read()?;
        let host = hosts
            .iter_mut()
            .find(|h| h.id == id.as_str())
            .ok_or_else(|| missing(id))?;
        host.name = name.as_str().to_owned();
        let host = host.clone();
        self.write(&hosts)?;
        Ok(host)
    }
}

/// Twelve random lowercase letters and digits.
fn new_id() -> HostId {
    const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::rng();
    let text: String = (0..12)
        .map(|_| char::from(CHARS[rng.random_range(0..CHARS.len())]))
        .collect();
    HostId::parse(&text).unwrap_or_else(|_| unreachable!("made of allowed characters"))
}

fn missing(id: &HostId) -> ProtocolError {
    ProtocolError::new(
        ErrorCode::NotFound,
        format!("no remote machine {}", id.as_str()),
    )
}

fn internal(e: &dyn std::fmt::Display) -> ProtocolError {
    ProtocolError::new(ErrorCode::Internal, format!("remote machines: {e}"))
}

/// A folder only this user may read (made 0700 on Unix); a symlink is
/// refused.
fn private_dir(dir: &Path) -> ServiceResult<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(dir).map_err(|e| internal(&e))?;
    let meta = std::fs::symlink_metadata(dir).map_err(|e| internal(&e))?;
    if !meta.is_dir() {
        return Err(internal(&format!("{} is not a folder", dir.display())));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // A folder made before keeps its mode otherwise.
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| internal(&e))?;
    }
    Ok(())
}

/// Writes `path` whole or not at all, readable by this user only. The
/// temporary file is always new: never one left behind, never a link.
fn write_private(path: &Path, bytes: &[u8]) -> ServiceResult<()> {
    use std::io::Write;
    let tmp = path.with_extension("tmp");
    match std::fs::remove_file(&tmp) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(internal(&e)),
        _ => {}
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&tmp).map_err(|e| internal(&e))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| internal(&e))?;
    std::fs::rename(&tmp, path).map_err(|e| internal(&e))
}

/// The arguments for `ssh` (without the program) that reach `daemon` on
/// `destination`, trusting only the keys in `pins` under `ovra-<id>`.
///
/// OpenSSH takes the first value it sees for an option and reads the
/// command line before `~/.ssh/config`, so each of these wins over the
/// person's config. Everything else in their config still applies.
pub fn arguments(
    pins: &Path,
    id: &HostId,
    destination: &Destination,
    remote: Remote<'_>,
) -> Result<Vec<String>, ProtocolError> {
    let pins = option_path(pins)?;
    let mut args: Vec<String> = ["-T", "-x", "-a"].map(String::from).to_vec();
    let options = [
        // Must come before the option a stock OpenSSH does not know.
        "IgnoreUnknown=GSSAPIKeyExchange".to_owned(),
        "GSSAPIKeyExchange=no".to_owned(),
        "BatchMode=yes".to_owned(),
        "ConnectTimeout=20".to_owned(),
        "StrictHostKeyChecking=yes".to_owned(),
        "UpdateHostKeys=no".to_owned(),
        format!("HostKeyAlias=ovra-{}", id.as_str()),
        format!("UserKnownHostsFile={pins}"),
        "GlobalKnownHostsFile=none".to_owned(),
        "KnownHostsCommand=none".to_owned(),
        "VerifyHostKeyDNS=no".to_owned(),
        "CheckHostIP=no".to_owned(),
        "ForwardAgent=no".to_owned(),
        "ForwardX11=no".to_owned(),
        "ForwardX11Trusted=no".to_owned(),
        "GSSAPIDelegateCredentials=no".to_owned(),
        "ClearAllForwardings=yes".to_owned(),
        "Tunnel=no".to_owned(),
        "PermitLocalCommand=no".to_owned(),
        "RemoteCommand=none".to_owned(),
        "RequestTTY=no".to_owned(),
        "SessionType=default".to_owned(),
        "StdinNull=no".to_owned(),
        "ForkAfterAuthentication=no".to_owned(),
        "EscapeChar=none".to_owned(),
        "ControlMaster=no".to_owned(),
        "ControlPath=none".to_owned(),
        // Only replaces a SetEnv in the person's config; nothing reads it.
        "SetEnv=OVRA_UNUSED=1".to_owned(),
        "ServerAliveInterval=15".to_owned(),
        "ServerAliveCountMax=4".to_owned(),
    ];
    for option in options {
        args.push("-o".to_owned());
        args.push(option);
    }
    args.push("--".to_owned());
    args.push(destination.as_str().to_owned());
    match remote {
        Remote::Attach(daemon) => args.extend([daemon.as_str().to_owned(), "--attach".to_owned()]),
        Remote::Restart(daemon) => args.extend([
            daemon.as_str().to_owned(),
            "--attach".to_owned(),
            "--restart".to_owned(),
        ]),
        Remote::Uname => args.push(install::UNAME.to_owned()),
        Remote::Install(sha) => args.push(install::command(sha)),
    }
    Ok(args)
}

/// What `ssh` runs on the machine: always one of these fixed commands,
/// never text from the person or the machine.
#[derive(Debug, Clone, Copy)]
pub enum Remote<'a> {
    /// `<daemon> --attach`: the machine's core, started when none runs.
    Attach(&'a DaemonPath),
    /// `<daemon> --attach --restart`: the running core quits first, so the
    /// installed build starts; its agents stop.
    Restart(&'a DaemonPath),
    /// `uname -sm`: which build the machine needs.
    Uname,
    /// The upload of this laptop's build ([`install::command`]).
    Install(&'a install::Sha256),
}

/// The options of `args`, up to and with the destination: what `ssh -G`
/// checks.
fn options(args: &[String]) -> &[String] {
    let end = args
        .iter()
        .position(|a| a == "--")
        .map_or(args.len(), |at| (at + 2).min(args.len()));
    &args[..end]
}

/// A path as one value of a list option: quoted, because
/// `UserKnownHostsFile` splits on spaces and backslash escapes (every
/// Windows path has them), with `\` and `%` doubled, because OpenSSH
/// unescapes the one and expands `%` tokens. `$` has no escape (OpenSSH
/// expands `${NAME}`), so a path with one is refused.
fn option_path(path: &Path) -> Result<String, ProtocolError> {
    let text = path
        .to_str()
        .filter(|text| path.is_absolute() && !text.contains(['"', '$', '\n', '\r', '\0']));
    let Some(text) = text else {
        return Err(ProtocolError::new(
            ErrorCode::Unavailable,
            "Ovra's data folder has a name SSH cannot be given safely",
        ));
    };
    Ok(format!(
        "\"{}\"",
        text.replace('\\', "\\\\").replace('%', "%%")
    ))
}

#[cfg(test)]
mod tests;
