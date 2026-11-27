//! The servers of a project's `.mcp.json` that the person turns on for
//! Ovra's agents (`docs/specs/project-mcp.md`). A server is off until the
//! person turns it on; that pins its definition as written then, and a
//! session gets it only while `.mcp.json` still says exactly that. Ovra
//! passes the pinned definitions itself (`--mcp-config` with
//! `--strict-mcp-config`), so nothing else in the file loads. An HTTP
//! server's host goes on the project's netproxy allowlist while it is on.

use std::path::PathBuf;

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
    let plain = !name.is_empty()
        && name.len() <= MAX_NAME
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !plain {
        return Some("Ovra gives agents only servers named with letters, digits, - and _.".into());
    }
    if name == IDE_SERVER {
        return Some("This name is taken by Ovra's own tools.".into());
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
    let kind = text("type").unwrap_or(if server.contains_key("command") {
        "stdio"
    } else {
        "http"
    });
    match kind {
        "stdio" if text("command").is_some_and(|c| !c.trim().is_empty()) => None,
        "http" | "sse" => match text("url").map(host) {
            Some(Ok(_)) => None,
            Some(Err(why)) => Some(why.into()),
            None => Some("Has no url.".into()),
        },
        _ => Some("Ovra cannot run this kind of server.".into()),
    }
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
fn host(url: &str) -> Result<AllowedHost, &'static str> {
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
        host: definition
            .get("url")
            .and_then(Value::as_str)
            .and_then(|u| host(u).ok())
            .map(|h| h.host),
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

pub(crate) async fn set_server(
    core: &Core,
    project: &ProjectId,
    name: &str,
    on: bool,
) -> Result<(), ProtocolError> {
    let store = core.project_store();
    let mut pins = store.servers(project)?;
    if !on {
        if let Some(pin) = pins.servers.remove(name) {
            store.set_servers(project, &pins)?;
            if let Some(added) = pin.added_host {
                change_hosts(core, project, |hosts| hosts.retain(|h| h.host != added))?;
            }
        }
        return Ok(());
    }
    let current = definitions(core, core.root(project)?).await;
    let definition = current.get(name).ok_or_else(|| {
        ProtocolError::new(ErrorCode::NotFound, format!("{FILE} has no server {name}"))
    })?;
    if let Some(why) = blocked(name, definition) {
        return Err(ProtocolError::new(ErrorCode::BadRequest, why));
    }
    let old = pins.servers.remove(name).unwrap_or_default();
    let mut added_host = old.added_host;
    if let Some(wanted) = definition
        .get("url")
        .and_then(Value::as_str)
        .and_then(|u| host(u).ok())
    {
        let listed = store
            .allowlist(project)?
            .hosts
            .iter()
            .any(|h| h.host == wanted.host);
        if !listed {
            added_host = Some(wanted.host.clone());
            change_hosts(core, project, |hosts| hosts.push(wanted))?;
        }
    }
    // A changed server's old tools may be gone; its choices stay for those that are not.
    pins.servers.insert(
        name.to_owned(),
        ServerPin {
            definition: compact(definition),
            tools: old.tools,
            tools_off: old.tools_off,
            added_host,
        },
    );
    store.set_servers(project, &pins)
}

pub(crate) fn set_tool(
    core: &Core,
    project: &ProjectId,
    server: &str,
    tool: &str,
    on: bool,
) -> Result<(), ProtocolError> {
    let store = core.project_store();
    let mut pins = store.servers(project)?;
    let pin = pins.servers.get_mut(server).ok_or_else(|| {
        ProtocolError::new(ErrorCode::NotFound, format!("{server} is not turned on"))
    })?;
    pin.tools_off.retain(|t| t != tool);
    if !on {
        pin.tools_off.push(tool.to_owned());
    }
    store.set_servers(project, &pins)
}

/// A session listed its tools (`mcp__<server>__<tool>`): each turned-on
/// server keeps its own, so the person can turn one off.
pub(crate) fn tools_listed(core: &Core, project: &ProjectId, tools: &[String]) {
    let store = core.project_store();
    let Ok(mut pins) = store.servers(project) else {
        return;
    };
    let mut changed = false;
    for (name, pin) in &mut pins.servers {
        let prefix = format!("mcp__{name}__");
        let mut listed: Vec<String> = tools
            .iter()
            .filter_map(|t| t.strip_prefix(&prefix))
            .filter(|t| !t.is_empty())
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
