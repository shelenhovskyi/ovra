//! The `browser_*` tools: the caller's own session's browser
//! (`Core::browser_open` and its siblings). Everything a page produces
//! (titles, URLs it went to, snapshots, console, network, elements)
//! reaches the agent wrapped as untrusted data from the page's origin.

use std::time::Duration;

use ovra_core::Core;
use ovra_core::protocol::browser::{MouseButton, PageId, PageInfo};
use ovra_core::services::{PageAction, ToolCaller};
use ovra_core::untrusted::untrusted;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::calls::{failed, parse, text};

/// The longest `browser_wait_for`.
const MAX_WAIT_MS: u64 = 30_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Open {
    url: String,
    width: Option<u32>,
    height: Option<u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Page {
    page_id: String,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum Step {
    Back,
    Forward,
    Reload,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Navigate {
    page_id: String,
    url: Option<String>,
    action: Option<Step>,
}

#[derive(Deserialize, Clone, Copy)]
enum Modifier {
    Alt,
    Control,
    Meta,
    Shift,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Click {
    page_id: String,
    #[serde(rename = "ref")]
    target: String,
    button: Option<MouseButton>,
    #[serde(default)]
    double: bool,
    #[serde(default)]
    modifiers: Vec<Modifier>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Type {
    page_id: String,
    #[serde(rename = "ref")]
    target: String,
    text: String,
    #[serde(default)]
    submit: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Press {
    page_id: String,
    key: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WaitFor {
    page_id: String,
    text: Option<String>,
    text_gone: Option<String>,
    timeout_ms: u64,
}

/// Runs the browser tool `name`: the MCP content of its answer, or the
/// text of a tool error. `None` when there is no such tool.
pub(crate) async fn call(
    core: &Core,
    caller: &ToolCaller,
    name: &str,
    arguments: Value,
) -> Option<Result<Vec<Value>, String>> {
    let answer = match name {
        "browser_open" => open(core, caller, arguments).await,
        "browser_navigate" => navigate(core, caller, arguments).await,
        "browser_click" | "browser_type" | "browser_press_key" | "browser_wait_for" => {
            act(core, caller, name, arguments).await
        }
        "browser_close" => close(core, caller, arguments).await,
        _ => return None,
    };
    Some(answer)
}

async fn open(core: &Core, caller: &ToolCaller, arguments: Value) -> Result<Vec<Value>, String> {
    let Open { url, width, height } = parse(arguments)?;
    let size = match (width, height) {
        (None, None) => None,
        (w, h) => Some((w.unwrap_or(1280), h.unwrap_or(800))),
    };
    let page = core.browser_open(caller, url, size).await.map_err(failed)?;
    Ok(content(&page_json(&page)))
}

async fn navigate(
    core: &Core,
    caller: &ToolCaller,
    arguments: Value,
) -> Result<Vec<Value>, String> {
    let args: Navigate = parse(arguments)?;
    let action = match (args.url, args.action) {
        (Some(url), None) => PageAction::Navigate { url },
        (None, Some(Step::Back)) => PageAction::Back,
        (None, Some(Step::Forward)) => PageAction::Forward,
        (None, Some(Step::Reload)) => PageAction::Reload,
        _ => return Err("name exactly one of url or action".to_owned()),
    };
    let page = PageId(args.page_id);
    let after = core
        .browser_act(caller, &page, action)
        .await
        .map_err(failed)?;
    Ok(content(&page_json(&after)))
}

async fn act(
    core: &Core,
    caller: &ToolCaller,
    name: &str,
    arguments: Value,
) -> Result<Vec<Value>, String> {
    let (page, action) = match name {
        "browser_click" => {
            let args: Click = parse(arguments)?;
            let modifiers = args.modifiers.iter().fold(0, |bits, m| {
                bits | match m {
                    Modifier::Alt => 1,
                    Modifier::Control => 2,
                    Modifier::Meta => 4,
                    Modifier::Shift => 8,
                }
            });
            let action = PageAction::Click {
                target: args.target,
                button: args.button.unwrap_or(MouseButton::Left),
                double: args.double,
                modifiers,
            };
            (args.page_id, action)
        }
        "browser_type" => {
            let args: Type = parse(arguments)?;
            let action = PageAction::Type {
                target: args.target,
                text: args.text,
                submit: args.submit,
            };
            (args.page_id, action)
        }
        "browser_press_key" => {
            let Press { page_id, key } = parse(arguments)?;
            (page_id, PageAction::Press { key })
        }
        _ => {
            let args: WaitFor = parse(arguments)?;
            if args.timeout_ms > MAX_WAIT_MS {
                return Err(format!("timeout_ms is at most {MAX_WAIT_MS}"));
            }
            let action = PageAction::WaitFor {
                text: args.text,
                text_gone: args.text_gone,
                timeout: Duration::from_millis(args.timeout_ms),
            };
            (args.page_id, action)
        }
    };
    let after = core
        .browser_act(caller, &PageId(page), action)
        .await
        .map_err(failed)?;
    Ok(content(&page_json(&after)))
}

async fn close(core: &Core, caller: &ToolCaller, arguments: Value) -> Result<Vec<Value>, String> {
    let Page { page_id } = parse(arguments)?;
    let page = PageId(page_id);
    core.browser_close(caller, &page).await.map_err(failed)?;
    Ok(vec![text_block(format!("Closed {}.", page.0))])
}

/// `scheme://host[:port]` of the page: the origin its content is from.
pub(crate) fn origin(url: &str) -> String {
    let start = url.find("://").map_or(0, |i| i + 3);
    let end = url[start..]
        .find(['/', '?', '#'])
        .map_or(url.len(), |i| start + i);
    url[..end].to_owned()
}

/// A page's id, and its address and title as data from it.
fn page_json(page: &PageInfo) -> Value {
    let shown = format!("url: {}\ntitle: {}", page.url, page.title);
    json!({"page_id": page.id, "page": untrusted(&origin(&page.url), &shown)})
}

fn content(value: &Value) -> Vec<Value> {
    vec![text_block(text(value))]
}

fn text_block(text: String) -> Value {
    json!({"type": "text", "text": text})
}
