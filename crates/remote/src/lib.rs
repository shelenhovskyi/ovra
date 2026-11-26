//! Phone access over the LAN (ADR 0014, `docs/specs/companion.md`): one
//! listener on one private address of this computer. TLS with Ovra's own
//! name-constrained CA for everything except the CA download; pairing by a
//! single-use QR secret that the person confirms on the computer; each
//! WebSocket proves the phone's device key by signing a challenge, and gets
//! a phone client of the core (`Core::connect_phone`).

mod auth;
mod ca;
mod devices;
mod net;
mod phone;
mod server;

use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ovra_core::Core;
use ovra_core::protocol::remote::{
    Device, PAIRING_SECONDS, Pairing, PairingRequest, RemoteEvent, RemoteStatus,
};
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{RemoteAccess, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, oneshot};
use tokio_util::sync::CancellationToken;

pub use server::Asset;

/// Phone access turns itself off after this long without a phone.
const IDLE_OFF_MS: u64 = 7 * 24 * 60 * 60 * 1000;

/// Where the mobile app's files come from: a folder (the daemon's
/// `ui/dist`) or the app's embedded assets.
pub type Assets = Arc<dyn Fn(&str) -> Option<Asset> + Send + Sync>;

/// Serves `dir`, e.g. `ui/dist`, for the mobile app at `mobile.html`.
pub fn assets_from_dir(dir: PathBuf) -> Assets {
    Arc::new(move |path: &str| {
        let path = path.trim_start_matches('/');
        let bytes = std::fs::read(dir.join(inside(path)?)).ok()?;
        Some(Asset::new(path, bytes))
    })
}

/// `path` as a relative path of plain names only: no `..`, root, drive
/// (`C:`) or backslash, which could leave the folder on some system.
fn inside(path: &str) -> Option<&Path> {
    let plain = !path.is_empty()
        && !path.contains(['\\', ':'])
        && Path::new(path)
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)));
    plain.then(|| Path::new(path))
}

/// The real [`RemoteAccess`].
pub struct RemoteServer {
    shared: Arc<Shared>,
}

impl std::fmt::Debug for RemoteServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteServer").finish_non_exhaustive()
    }
}

pub(crate) struct Shared {
    dir: PathBuf,
    assets: Assets,
    /// Tests only: lets the listener use 127.0.0.1.
    loopback: bool,
    state: Mutex<State>,
    /// A device id (or `*` for all) whose connections must close now.
    kicks: broadcast::Sender<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct Settings {
    enabled: bool,
    #[serde(default)]
    address: Option<String>,
    #[serde(default)]
    port: Option<u16>,
    /// When it was turned on, for the idle rule.
    #[serde(default)]
    enabled_at_ms: u64,
}

pub(crate) struct State {
    settings: Settings,
    devices: devices::Devices,
    running: Option<Running>,
    problem: Option<String>,
    pairing: Option<Window>,
    connected: Vec<String>,
    limits: server::Limits,
}

pub(crate) struct Running {
    pub(crate) core: Core,
    pub(crate) ip: IpAddr,
    pub(crate) port: u16,
    pub(crate) ca: Arc<ca::Ca>,
    stop: CancellationToken,
}

impl Running {
    pub(crate) fn origin(&self) -> String {
        let host = match self.ip {
            IpAddr::V4(ip) => ip.to_string(),
            IpAddr::V6(ip) => format!("[{ip}]"),
        };
        format!("https://{host}:{}", self.port)
    }
    pub(crate) fn host(&self) -> String {
        self.origin().trim_start_matches("https://").to_owned()
    }
}

pub(crate) struct Window {
    pub(crate) secret: String,
    pub(crate) until: Instant,
    pub(crate) waiting: Option<Waiting>,
}

pub(crate) struct Waiting {
    pub(crate) id: String,
    /// Taken by the answer; the request stays until the pairing ends.
    pub(crate) answer: Option<oneshot::Sender<bool>>,
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn lock(m: &Mutex<State>) -> std::sync::MutexGuard<'_, State> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn err(code: ErrorCode, message: impl Into<String>) -> ProtocolError {
    ProtocolError::new(code, message)
}

impl RemoteServer {
    /// Keeps its CA, settings and devices in `dir` (e.g. `<data>/remote`,
    /// a protected path) and serves the mobile app from `assets`.
    pub fn new(dir: &Path, assets: Assets) -> Self {
        Self::build(dir, assets, false)
    }

    fn build(dir: &Path, assets: Assets, loopback: bool) -> Self {
        let settings = std::fs::read(dir.join("settings.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let devices = devices::Devices::load(dir.join("devices.json"), now_ms());
        let (kicks, _) = broadcast::channel(16);
        Self {
            shared: Arc::new(Shared {
                dir: dir.to_owned(),
                assets,
                loopback,
                state: Mutex::new(State {
                    settings,
                    devices,
                    running: None,
                    problem: None,
                    pairing: None,
                    connected: Vec::new(),
                    limits: server::Limits::default(),
                }),
                kicks,
            }),
        }
    }

    /// For tests in this crate: may listen on 127.0.0.1.
    #[cfg(test)]
    pub(crate) fn for_tests(dir: &Path, assets: Assets) -> Self {
        Self::build(dir, assets, true)
    }

    /// Starts listening again if the person left phone access on, unless
    /// no phone came for [`IDLE_OFF_MS`]. Call once, inside the runtime,
    /// after the core is built.
    pub fn restore(&self, core: Core) {
        let settings = lock(&self.shared.state).settings.clone();
        if !settings.enabled {
            return;
        }
        if self.idle_too_long() {
            let _ = self.set_enabled(false, None);
            return;
        }
        if let Err(e) = self.enable(core, settings.address) {
            tracing::warn!(error = %e, "phone access did not start again");
        }
    }

    fn idle_too_long(&self) -> bool {
        let state = lock(&self.shared.state);
        let last_seen = state
            .devices
            .list()
            .iter()
            .filter_map(|d| d.seen_at_ms)
            .max()
            .unwrap_or(0);
        let last = last_seen.max(state.settings.enabled_at_ms);
        now_ms().saturating_sub(last) > IDLE_OFF_MS
    }

    fn set_enabled(
        &self,
        enabled: bool,
        address: Option<String>,
    ) -> std::result::Result<(), String> {
        let mut state = lock(&self.shared.state);
        // Only turning it on starts the idle clock; a restart does not.
        if enabled && !state.settings.enabled {
            state.settings.enabled_at_ms = now_ms();
        }
        state.settings.enabled = enabled;
        if enabled {
            state.settings.address = address;
        }
        save_settings(&self.shared.dir, &state.settings)
    }

    fn status_of(&self, state: &State) -> RemoteStatus {
        self.shared.status_of(state)
    }

    /// Stops listening; the caller says so (`remote.changed`), the core
    /// for requests and [`maintain`] for its own.
    fn stop(&self) {
        let running = lock(&self.shared.state).running.take();
        if let Some(running) = running {
            running.stop.cancel();
            let _ = self.shared.kicks.send("*".into());
        }
        lock(&self.shared.state).pairing = None;
    }
}

/// While listening, once an hour: forgets phones unseen for 30 days,
/// turns phone access off after [`IDLE_OFF_MS`] without a phone, and
/// starts again with a new leaf before the old one runs out.
async fn maintain(shared: Arc<Shared>, stop: CancellationToken) {
    let started = Instant::now();
    let mut hourly = tokio::time::interval(Duration::from_secs(60 * 60));
    hourly.tick().await;
    loop {
        tokio::select! {
            () = stop.cancelled() => return,
            _ = hourly.tick() => {}
        }
        let server = RemoteServer {
            shared: shared.clone(),
        };
        let (core, address) = {
            let mut state = shared.lock();
            state.devices.expire(now_ms());
            let Some(running) = &state.running else {
                return;
            };
            (running.core.clone(), state.settings.address.clone())
        };
        if server.idle_too_long() {
            tracing::info!("phone access turned off: no phone for 7 days");
            if let Ok(status) = server.disable() {
                core.emit(RemoteEvent::Changed(status));
            }
            return;
        }
        if started.elapsed() > ca::RENEW_AFTER {
            match server.enable(core.clone(), address) {
                Ok(status) => core.emit(RemoteEvent::Changed(status)),
                Err(e) => tracing::warn!(error = %e, "phone access did not renew its certificate"),
            }
            return;
        }
    }
}

fn save_settings(dir: &Path, settings: &Settings) -> std::result::Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_vec_pretty(settings).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("settings.json"), json).map_err(|e| e.to_string())
}

impl RemoteAccess for RemoteServer {
    fn status(&self) -> Result<RemoteStatus> {
        Ok(self.status_of(&lock(&self.shared.state)))
    }

    fn enable(&self, core: Core, address: Option<String>) -> Result<RemoteStatus> {
        let addresses = net::private_addresses(self.shared.loopback);
        let ip = match &address {
            Some(a) => a
                .parse::<IpAddr>()
                .ok()
                .filter(|ip| addresses.contains(ip))
                .ok_or_else(|| {
                    err(
                        ErrorCode::BadRequest,
                        format!("{a} is not a private address of this computer"),
                    )
                })?,
            None => *addresses.first().ok_or_else(|| {
                err(
                    ErrorCode::Unavailable,
                    "this computer has no private network address; connect to a home network first",
                )
            })?,
        };
        self.stop();
        let ca = Arc::new(ca::Ca::load_or_create(&self.shared.dir).map_err(|e| {
            err(
                ErrorCode::Internal,
                format!("making Ovra's certificate: {e}"),
            )
        })?);
        let tls = ca
            .server_for(ip)
            .map_err(|e| err(ErrorCode::Internal, format!("making the certificate: {e}")))?;
        let wanted = lock(&self.shared.state).settings.port;
        let listener = net::bind(ip, wanted)
            .map_err(|e| err(ErrorCode::Unavailable, format!("listening on {ip}: {e}")))?;
        let port = listener
            .local_addr()
            .map(|a: SocketAddr| a.port())
            .map_err(|e| err(ErrorCode::Internal, e.to_string()))?;
        let stop = CancellationToken::new();
        {
            let mut state = lock(&self.shared.state);
            state.settings.port = Some(port);
            state.problem = None;
            state.running = Some(Running {
                core: core.clone(),
                ip,
                port,
                ca,
                stop: stop.clone(),
            });
        }
        self.set_enabled(true, address)
            .map_err(|e| err(ErrorCode::Internal, e))?;
        server::spawn(self.shared.clone(), listener, tls, stop.clone());
        tokio::spawn(maintain(self.shared.clone(), stop));
        tracing::info!(%ip, port, "phone access listening");
        self.status()
    }

    fn disable(&self) -> Result<RemoteStatus> {
        self.stop();
        self.set_enabled(false, None)
            .map_err(|e| err(ErrorCode::Internal, e))?;
        self.status()
    }

    fn start_pairing(&self) -> Result<Pairing> {
        let mut state = lock(&self.shared.state);
        let Some(running) = &state.running else {
            return Err(err(ErrorCode::Conflict, "phone access is off"));
        };
        let secret = auth::random_token();
        let origin = running.origin();
        let url = format!("{origin}/#pair={secret}");
        let certificate_url = format!("{}/ca.crt", origin.replacen("https://", "http://", 1));
        let fingerprint = running.ca.fingerprint();
        let qr_svg = qrcode::QrCode::new(url.as_bytes())
            .map_err(|e| err(ErrorCode::Internal, e.to_string()))?
            .render::<qrcode::render::svg::Color<'_>>()
            .min_dimensions(220, 220)
            .build();
        state.pairing = Some(Window {
            secret,
            until: Instant::now() + Duration::from_secs(PAIRING_SECONDS),
            waiting: None,
        });
        Ok(Pairing {
            url,
            qr_svg,
            certificate_url,
            certificate_fingerprint: fingerprint,
            expires_in: PAIRING_SECONDS,
        })
    }

    fn answer_pairing(&self, id: &str, allow: bool) -> Result<()> {
        let mut state = lock(&self.shared.state);
        let answer = state
            .pairing
            .as_mut()
            .and_then(|w| w.waiting.as_mut())
            .filter(|w| w.id == id)
            .and_then(|w| w.answer.take());
        let Some(answer) = answer else {
            return Err(err(ErrorCode::NotFound, "no phone waits for this answer"));
        };
        let _ = answer.send(allow);
        Ok(())
    }

    fn devices(&self) -> Result<Vec<Device>> {
        let state = lock(&self.shared.state);
        Ok(state
            .devices
            .list()
            .iter()
            .map(|d| Device {
                id: d.id.clone(),
                name: d.name.clone(),
                paired_at_ms: d.paired_at_ms,
                seen_at_ms: d.seen_at_ms,
                connected: state.connected.contains(&d.id),
            })
            .collect())
    }

    fn forget(&self, device: &str) -> Result<()> {
        self.shared.forget(device)
    }
}

impl Shared {
    fn status_of(&self, state: &State) -> RemoteStatus {
        RemoteStatus {
            enabled: state.settings.enabled,
            url: state.running.as_ref().map(Running::origin),
            addresses: net::private_addresses(self.loopback)
                .iter()
                .map(IpAddr::to_string)
                .collect(),
            connected: state.connected.len() as u32,
            problem: state.problem.clone(),
        }
    }

    pub(crate) fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        lock(&self.state)
    }

    /// Forgets `device` and closes its sessions.
    pub(crate) fn forget(&self, device: &str) -> Result<()> {
        let forgotten = self
            .lock()
            .devices
            .forget(device)
            .map_err(|e| err(ErrorCode::Internal, e))?;
        if !forgotten {
            return Err(err(
                ErrorCode::NotFound,
                format!("device {device} not found"),
            ));
        }
        tracing::info!(device, "phone forgotten");
        let _ = self.kicks.send(device.to_owned());
        let core = self.lock().running.as_ref().map(|r| r.core.clone());
        if let Some(core) = core {
            core.emit(RemoteEvent::DevicesChanged);
        }
        Ok(())
    }

    /// A phone used the pairing secret: asks the person and waits. `None`
    /// when the secret is wrong, spent or out of time; `Some(None)` when
    /// the person said no or did not answer.
    pub(crate) async fn pair(
        &self,
        secret: &str,
        key: &str,
        name: &str,
    ) -> Option<Option<devices::Stored>> {
        let (tx, rx) = oneshot::channel();
        let id = auth::random_token()[..12].to_owned();
        let (core, code) = {
            let mut state = self.lock();
            let core = state.running.as_ref()?.core.clone();
            let window = state.pairing.as_mut()?;
            if window.until < Instant::now()
                || window.waiting.is_some()
                || !auth::same(&window.secret, secret)
            {
                return None;
            }
            // Spent on first use, whatever the person answers.
            window.secret = auth::random_token();
            window.waiting = Some(Waiting {
                id: id.clone(),
                answer: Some(tx),
            });
            (core, auth::code(secret, key))
        };
        core.emit(RemoteEvent::PairingRequest(PairingRequest {
            id: id.clone(),
            name: name.to_owned(),
            code,
        }));
        let allowed = tokio::time::timeout(Duration::from_secs(120), rx)
            .await
            .ok()
            .and_then(std::result::Result::ok)
            .unwrap_or(false);
        let (device, closed) = {
            let mut state = self.lock();
            // Only this request's window: the person may have opened a new
            // one meanwhile.
            let closed = state
                .pairing
                .as_ref()
                .and_then(|w| w.waiting.as_ref())
                .is_some_and(|w| w.id == id);
            if closed {
                state.pairing = None;
            }
            let device = allowed
                .then(|| devices::Stored {
                    id: auth::random_token()[..16].to_owned(),
                    name: name.to_owned(),
                    key: key.to_owned(),
                    paired_at_ms: now_ms(),
                    seen_at_ms: None,
                })
                .and_then(|device| {
                    let gone = state.devices.add(device.clone()).ok()?;
                    // A phone dropped for the newcomer loses its sessions too.
                    for id in gone {
                        let _ = self.kicks.send(id);
                    }
                    Some(device)
                });
            (device, closed)
        };
        if closed {
            core.emit(RemoteEvent::PairingClosed);
        }
        match &device {
            Some(d) => tracing::info!(device = %d.id, name = %d.name, "phone paired"),
            None => tracing::info!(name, "phone not paired"),
        }
        if device.is_some() {
            core.emit(RemoteEvent::DevicesChanged);
        }
        Some(device)
    }

    pub(crate) fn connected(&self, device: &str, on: bool) {
        let core = {
            let mut state = self.lock();
            if on {
                tracing::info!(device, "phone connected");
                state.connected.push(device.to_owned());
                state.devices.seen(device, now_ms());
            } else if let Some(i) = state.connected.iter().position(|d| d == device) {
                state.connected.remove(i);
            }
            let status = self.status_of(&state);
            state.running.as_ref().map(|r| (r.core.clone(), status))
        };
        if let Some((core, status)) = core {
            core.emit(RemoteEvent::Changed(status));
            core.emit(RemoteEvent::DevicesChanged);
        }
    }
}

#[cfg(test)]
mod tests;
