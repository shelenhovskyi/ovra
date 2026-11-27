//! The servers of a project's `.mcp.json` that the person turns on for
//! Ovra's agents (`docs/specs/project-mcp.md`). A server is off until the
//! person turns it on; that pins its definition as written then, and a
//! session gets it only while `.mcp.json` still says exactly that. Ovra
//! passes the pinned definitions itself (`--mcp-config` with
//! `--strict-mcp-config`), so nothing else in the file loads. An HTTP
//! server's host goes on the project's netproxy allowlist while it is on.

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, PoisonError};

use serde_json::{Map, Value};

use crate::Core;
use crate::path::RelPath;
use crate::protocol::catalog::{ServerPin, ServerPins, ServerTool, ServerUse};
use crate::protocol::fs::FileBody;
use crate::protocol::netproxy::AllowedHost;
use crate::protocol::projects::ProjectId;
use crate::protocol::{ErrorCode, ProtocolError};
use crate::services::ProjectServer;

pub(crate) const FILE: &str = ".mcp.json";
/// The name of Ovra's own server in a session's `--mcp-config`.
const IDE_SERVER: &str = "ovra";
/// Longer server names are not given to agents.
const MAX_NAME: usize = 64;
/// Tools kept per server.
const MAX_TOOLS: usize = 200;

/// Held while the stored pins are read, changed and written back, so a
/// session listing its tools cannot undo a tool the person turned off.
static PINS: Mutex<()> = Mutex::new(());

fn pins_held() -> MutexGuard<'static, ()> {
    PINS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The `mcpServers` of the project's `.mcp.json`; empty when it has none
/// or cannot be read.
pub(crate) async fn definitions(core: &Core, root: PathBuf) -> Map<String, Value> {
    let Ok(path) = RelPath::parse(FILE) else {
        return Map::new();
    };
    let Ok(file) = core.fs_service().read_file(root, path).await else {
        return Map::new();
    };
    let FileBody::Text { text } = file.body else {
        return Map::new();
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(mut json)) => match json.remove("mcpServers") {
            Some(Value::Object(servers)) => servers,
            _ => Map::new(),
        },
        _ => Map::new(),
    }
}

/// The definition as Ovra pins and compares it.
fn compact(definition: &Value) -> String {
    definition.to_string()
}

/// Why Ovra does not give this server to agents, in words; `None` if it can.
pub(crate) fn blocked(name: &str, definition: &Value) -> Option<String> {
    // `__` separates the server from the tool in `mcp__<server>__<tool>`.
    if !plain(name) || name.len() > MAX_NAME || name.contains("__") {
        return Some(
            "Ovra gives agents only servers named with letters, digits, - and single _.".into(),
        );
    }
    // Its tools would look like Ovra's own (`mcp__ovra__...`).
    if name.to_ascii_lowercase().starts_with(IDE_SERVER) {
        return Some("Names that start with ovra are kept for Ovra's own tools.".into());
    }
    let Some(server) = definition.as_object() else {
        return Some("Not a server definition.".into());
    };
    if server.get("disabled").and_then(Value::as_bool) == Some(true)
        || server.get("enabled").and_then(Value::as_bool) == Some(false)
    {
        return Some("Turned off in this file.".into());
    }
    let names = variables(&compact(definition));
    if !names.is_empty() {
        return Some(format!(
            "Needs {} from your environment, which Ovra does not give agents yet.",
            names.join(", ")
        ));
    }
    let text = |key: &str| server.get(key).and_then(Value::as_str);
    match kind(definition) {
        Some(Kind::Stdio) if text("command").is_some_and(|c| !c.trim().is_empty()) => None,
        Some(Kind::Remote) => match text("url").map(host) {
            Some(Ok(_)) => None,
            Some(Err(why)) => Some(why.into()),
            None => Some("Has no url.".into()),
        },
        _ => Some("Ovra cannot run this kind of server.".into()),
    }
}

/// Letters, digits, `-` and `_`, and something.
fn plain(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

#[derive(Debug, PartialEq, Eq)]
enum Kind {
    /// A command Claude starts inside the session's sandbox.
    Stdio,
    /// An `http` or `sse` server Claude reaches at its `url`.
    Remote,
}

/// How Claude reaches the server, as it reads `type`: stdio without one
/// when there is a `command`, else `http`. `None` for any other kind.
fn kind(definition: &Value) -> Option<Kind> {
    let server = definition.as_object()?;
    match server.get("type") {
        None if server.contains_key("command") => Some(Kind::Stdio),
        None => Some(Kind::Remote),
        Some(Value::String(t)) if t == "stdio" => Some(Kind::Stdio),
        Some(Value::String(t)) if t == "http" || t == "sse" => Some(Kind::Remote),
        Some(_) => None,
    }
}

/// The host a remote server reaches, as the allowlist keeps it; none for
/// a stdio server, whatever else its definition says.
fn server_host(definition: &Value) -> Option<AllowedHost> {
    if kind(definition) != Some(Kind::Remote) {
        return None;
    }
    host(definition.get("url")?.as_str()?).ok()
}

/// The names in `${NAME}` and `${NAME:-default}`, in order, once each.
fn variables(text: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for part in text.split("${").skip(1) {
        let name: String = part
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        let name = if name.is_empty() {
            "a variable".into()
        } else {
            name
        };
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// An HTTPS URL's host and port as the allowlist keeps them.
pub(super) fn host(url: &str) -> Result<AllowedHost, &'static str> {
    let rest = url
        .trim()
        .strip_prefix("https://")
        .ok_or("Ovra reaches servers over HTTPS only.")?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.contains('@') {
        return Err("Ovra does not pass a name and password in the url.");
    }
    let (name, port) = match authority.strip_prefix('[') {
        Some(rest) => {
            let (ip, after) = rest.split_once(']').ok_or("The url's host is not valid.")?;
            (ip, after.strip_prefix(':'))
        }
        None => match authority.split_once(':') {
            Some((name, port)) => (name, Some(port)),
            None => (authority, None),
        },
    };
    let port = match port {
        Some(port) => Some(
            port.parse::<u16>()
                .map_err(|_| "The url's port is not a number.")?,
        ),
        None => None,
    }
    .filter(|p| *p != 443);
    let host =
        crate::netproxy::parse_host(name).map_err(|_| "The url's host cannot be reached.")?;
    Ok(AllowedHost { host, port })
}

/// What the catalog shows for the server `name` of `.mcp.json`.
pub(crate) fn server_use(name: &str, definition: &Value, pins: &ServerPins) -> ServerUse {
    let pin = pins.servers.get(name);
    let blocked = blocked(name, definition);
    let same = pin.is_some_and(|p| p.definition == compact(definition));
    ServerUse {
        on: same && blocked.is_none(),
        changed: pin.is_some() && !same,
        host: server_host(definition).map(|h| match h.port {
            Some(port) => format!("{}:{port}", h.host),
            None => h.host,
        }),
        blocked,
        tools: pin
            .filter(|_| same)
            .map(|p| {
                p.tools
                    .iter()
                    .map(|t| ServerTool {
                        name: t.clone(),
                        on: !p.tools_off.contains(t),
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// The servers a session of `project` gets: those turned on and unchanged.
pub(crate) async fn for_session(core: &Core, project: &ProjectId) -> Vec<ProjectServer> {
    let Ok(pins) = core.project_store().servers(project) else {
        return Vec::new();
    };
    if pins.servers.is_empty() {
        return Vec::new();
    }
    let Ok(root) = core.root(project) else {
        return Vec::new();
    };
    let current = definitions(core, root).await;
    pins.servers
        .iter()
        .filter_map(|(name, pin)| {
            let definition = current.get(name)?;
            (compact(definition) == pin.definition && blocked(name, definition).is_none()).then(
                || ProjectServer {
                    name: name.clone(),
                    definition: definition.clone(),
                    tools_off: pin.tools_off.clone(),
                },
            )
        })
        .collect()
}

/// Turns the server `name` on or off. On pins its definition and puts its
/// host on the allowlist unless the list has it already; the host Ovra
/// added comes off again with the server, unless another turned-on server
/// still reaches it, which then keeps it.
pub(crate) async fn set_server(
    core: &Core,
    project: &ProjectId,
    name: &str,
    on: bool,
) -> Result<(), ProtocolError> {
    let current = definitions(core, core.root(project)?).await;
    let definition = current.get(name);
    if on {
        let definition = definition.ok_or_else(|| {
            ProtocolError::new(ErrorCode::NotFound, format!("{FILE} has no server {name}"))
        })?;
        if let Some(why) = blocked(name, definition) {
            return Err(ProtocolError::new(ErrorCode::BadRequest, why));
        }
    }
    let store = core.project_store();
    let _held = pins_held();
    let mut pins = store.servers(project)?;
    let old = pins.servers.remove(name);
    if !on && old.is_none() {
        return Ok(());
    }
    let old = old.unwrap_or_default();
    let wanted = definition.filter(|_| on).and_then(server_host);
    let mut added = old.added_host.clone();
    let mut release = None;
    if added.is_some() && added != wanted {
        release = added
            .take()
            .filter(|host| !hand_over(&mut pins, &current, host));
    }
    let mut add = None;
    if let Some(wanted) = wanted
        && added.is_none()
        && !store.allowlist(project)?.hosts.contains(&wanted)
    {
        added = Some(wanted.clone());
        add = Some(wanted);
    }
    if let Some(definition) = definition.filter(|_| on) {
        // A changed server's old tools may be gone; its choices stay for those that are not.
        pins.servers.insert(
            name.to_owned(),
            ServerPin {
                definition: compact(definition),
                tools: old.tools,
                tools_off: old.tools_off,
                added_host: added,
            },
        );
    }
    // The pin first: a host Ovra adds is always one it knows it added.
    store.set_servers(project, &pins)?;
    if add.is_some() || release.is_some() {
        change_hosts(core, project, |hosts| {
            hosts.retain(|h| Some(h) != release.as_ref());
            hosts.extend(add);
        })?;
    }
    Ok(())
}

/// Gives `host` to another turned-on server that reaches it now, which then
/// removes it in its turn; false when none does.
fn hand_over(pins: &mut ServerPins, current: &Map<String, Value>, host: &AllowedHost) -> bool {
    let other = pins.servers.iter_mut().find(|(name, pin)| {
        pin.added_host.is_none()
            && current.get(name.as_str()).and_then(server_host).as_ref() == Some(host)
    });
    match other {
        Some((_, pin)) => {
            pin.added_host = Some(host.clone());
            true
        }
        None => false,
    }
}

pub(crate) fn set_tool(
    core: &Core,
    project: &ProjectId,
    server: &str,
    tool: &str,
    on: bool,
) -> Result<(), ProtocolError> {
    let store = core.project_store();
    let _held = pins_held();
    let mut pins = store.servers(project)?;
    let pin = pins.servers.get_mut(server).ok_or_else(|| {
        ProtocolError::new(ErrorCode::NotFound, format!("{server} is not turned on"))
    })?;
    if !pin.tools.iter().any(|t| t == tool) {
        return Err(ProtocolError::new(
            ErrorCode::NotFound,
            format!("{server} has no tool {tool}"),
        ));
    }
    pin.tools_off.retain(|t| t != tool);
    if !on {
        pin.tools_off.push(tool.to_owned());
    }
    store.set_servers(project, &pins)
}

/// A session listed its tools (`mcp__<server>__<tool>`): each turned-on
/// server keeps its own, so the person can turn one off. Only plain names:
/// `--disallowedTools` splits at commas and spaces.
pub(crate) fn tools_listed(core: &Core, project: &ProjectId, tools: &[String]) {
    let store = core.project_store();
    let _held = pins_held();
    let Ok(mut pins) = store.servers(project) else {
        return;
    };
    let mut changed = false;
    for (name, pin) in &mut pins.servers {
        let prefix = format!("mcp__{name}__");
        let mut listed: Vec<String> = tools
            .iter()
            .filter_map(|t| t.strip_prefix(&prefix))
            .filter(|t| plain(t))
            .map(str::to_owned)
            .take(MAX_TOOLS)
            .collect();
        listed.sort();
        listed.dedup();
        if !listed.is_empty() && listed != pin.tools {
            pin.tools = listed;
            changed = true;
        }
    }
    if changed {
        // Best effort: the next session lists them again.
        let _ = store.set_servers(project, &pins);
    }
}

/// Changes the project's allowlist as `netproxy.set_allowlist` would.
fn change_hosts(
    core: &Core,
    project: &ProjectId,
    change: impl FnOnce(&mut Vec<AllowedHost>),
) -> Result<(), ProtocolError> {
    let mut allowlist = core.project_store().allowlist(project)?;
    change(&mut allowlist.hosts);
    crate::netproxy::replace(core, project, allowlist)
}
