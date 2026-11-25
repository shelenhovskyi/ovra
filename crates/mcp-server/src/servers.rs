//! The `server_*` tools: the caller's dev servers (`Core::server_start` and
//! its siblings). Their output is the server's, which project code writes,
//! so it reaches the agent wrapped as untrusted data.

use std::time::Duration;

use ovra_core::Core;
use ovra_core::protocol::servers::{LogPage, ServerId, ServerInfo};
use ovra_core::services::ToolCaller;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::calls::{Nothing, failed, parse, text};
use crate::untrusted::untrusted;

/// The longest `wait_ms`, and how long a start waits without one.
const MAX_WAIT_MS: u64 = 60_000;
const DEFAULT_WAIT_MS: u64 = 30_000;
/// The most lines one look at a server's log returns.
const MAX_LINES: usize = 500;
/// Lines of its output a start or restart answers with.
const FIRST_LINES: usize = 20;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Start {
    name: String,
    wait_ms: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Server {
    id: String,
}

/// Runs the server tool `name`; `None` when there is no such tool.
pub(crate) async fn call(
    core: &Core,
    caller: &ToolCaller,
    name: &str,
    arguments: Value,
) -> Option<Result<String, String>> {
    Some(match name {
        "server_list" => list(core, caller, arguments).await,
        "server_start" => start(core, caller, arguments).await,
        "server_stop" => stop(core, caller, arguments).await,
        _ => return None,
    })
}

async fn list(core: &Core, caller: &ToolCaller, arguments: Value) -> Result<String, String> {
    let Nothing {} = parse(arguments)?;
    let list = core.server_list(caller).await.map_err(failed)?;
    let definitions: Vec<Value> = list
        .definitions
        .iter()
        .map(|d| json!({"name": d.name, "port": d.port}))
        .collect();
    let servers: Vec<Value> = list.servers.iter().map(server_json).collect();
    let mut answer = json!({"definitions": definitions, "servers": servers});
    if let Some(error) = list.error {
        answer["error"] = json!(error);
    }
    Ok(text(&answer))
}

async fn start(core: &Core, caller: &ToolCaller, arguments: Value) -> Result<String, String> {
    let Start { name, wait_ms } = parse(arguments)?;
    let wait = wait(wait_ms)?;
    // A server started before keeps its log; answer with this start's lines.
    let list = core.server_list(caller).await.map_err(failed)?;
    let before = match list.servers.iter().find(|s| s.name == name) {
        Some(old) => log_end(core, caller, &old.id)?,
        None => 0,
    };
    let info = core
        .server_start(caller, &name, wait)
        .await
        .map_err(failed)?;
    started(core, caller, &info, before)
}

async fn stop(core: &Core, caller: &ToolCaller, arguments: Value) -> Result<String, String> {
    let Server { id } = parse(arguments)?;
    let info = core
        .server_stop(caller, &ServerId(id))
        .await
        .map_err(failed)?;
    Ok(text(&server_json(&info)))
}

fn wait(wait_ms: Option<u64>) -> Result<Duration, String> {
    let ms = wait_ms.unwrap_or(DEFAULT_WAIT_MS);
    if ms > MAX_WAIT_MS {
        return Err(format!("wait_ms is at most {MAX_WAIT_MS}"));
    }
    Ok(Duration::from_millis(ms))
}

/// A started server with the first lines it wrote after `since`.
fn started(
    core: &Core,
    caller: &ToolCaller,
    info: &ServerInfo,
    since: u64,
) -> Result<String, String> {
    let page = core
        .server_logs(caller, &info.id, since, FIRST_LINES, None)
        .map_err(failed)?;
    let mut answer = server_json(info);
    answer["output"] = json!(output(&info.name, &page));
    answer["next"] = json!(page.next);
    Ok(text(&answer))
}

/// The number of the server's last line, 0 for none.
fn log_end(core: &Core, caller: &ToolCaller, id: &ServerId) -> Result<u64, String> {
    let mut end = 0;
    loop {
        let page = core
            .server_logs(caller, id, end, MAX_LINES, None)
            .map_err(failed)?;
        if page.lines.is_empty() {
            return Ok(end);
        }
        end = page.next;
    }
}

/// The lines of `page`, one per line, as data from server `name`.
fn output(name: &str, page: &LogPage) -> String {
    let lines: Vec<&str> = page.lines.iter().map(|l| l.text.as_str()).collect();
    untrusted(&format!("server:{name}"), &lines.join("\n"))
}

fn server_json(info: &ServerInfo) -> Value {
    json!({
        "id": info.id,
        "name": info.name,
        "status": info.status,
        "port": info.port,
        "url": format!("http://localhost:{}/", info.port),
    })
}
