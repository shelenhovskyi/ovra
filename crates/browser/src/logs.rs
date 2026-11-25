//! A page's console messages and requests, kept in rings and numbered
//! from 1, from the CDP events of the `Runtime`, `Log` and `Network`
//! domains.

use std::collections::{HashMap, VecDeque};

use ovra_core::services::{ConsoleEntry, NetworkEntry};
use serde_json::Value;

/// Entries kept of each kind; older ones drop out.
const RING: usize = 1000;

/// A console message is cut to this many characters.
const TEXT_LIMIT: usize = 4096;

#[derive(Debug, Default)]
pub(crate) struct Logs {
    console: VecDeque<ConsoleEntry>,
    console_n: u64,
    network: VecDeque<NetworkEntry>,
    network_n: u64,
    /// CDP's request id to the number of its latest entry.
    requests: HashMap<String, u64>,
}

fn push<T>(ring: &mut VecDeque<T>, entry: T) {
    if ring.len() == RING {
        ring.pop_front();
    }
    ring.push_back(entry);
}

/// What `console.log` and friends print for one argument.
fn shown(arg: &Value) -> String {
    match arg.get("value") {
        Some(Value::String(s)) => s.clone(),
        Some(value) if !value.is_null() => value.to_string(),
        _ => arg["description"]
            .as_str()
            .or(arg["type"].as_str())
            .unwrap_or_default()
            .to_owned(),
    }
}

fn after<T: Clone>(ring: &VecDeque<T>, n: impl Fn(&T) -> u64, since: u64, limit: usize) -> Vec<T> {
    ring.iter()
        .filter(|e| n(e) > since)
        .take(limit)
        .cloned()
        .collect()
}

impl Logs {
    fn log(&mut self, level: &str, text: String) {
        self.console_n += 1;
        let entry = ConsoleEntry {
            n: self.console_n,
            level: level.to_owned(),
            text: text.chars().take(TEXT_LIMIT).collect(),
        };
        push(&mut self.console, entry);
    }

    fn entry(&mut self, id: &str) -> Option<&mut NetworkEntry> {
        let n = *self.requests.get(id)?;
        self.network.iter_mut().rev().find(|e| e.n == n)
    }

    /// Takes in one event; any other event is ignored.
    pub(crate) fn event(&mut self, method: &str, params: &Value) {
        match method {
            "Runtime.consoleAPICalled" => {
                let level = match params["type"].as_str().unwrap_or_default() {
                    "info" => "info",
                    "warning" => "warning",
                    "error" | "assert" => "error",
                    "debug" => "debug",
                    _ => "log",
                };
                let args = params["args"].as_array().into_iter().flatten();
                let text: Vec<String> = args.map(shown).collect();
                self.log(level, text.join(" "));
            }
            "Runtime.exceptionThrown" => {
                let details = &params["exceptionDetails"];
                let text = details["exception"]["description"]
                    .as_str()
                    .or(details["text"].as_str())
                    .unwrap_or("an exception");
                self.log("error", text.to_owned());
            }
            "Log.entryAdded" => {
                let entry = &params["entry"];
                let level = match entry["level"].as_str().unwrap_or_default() {
                    "verbose" => "debug",
                    "warning" => "warning",
                    "error" => "error",
                    _ => "info",
                };
                self.log(level, entry["text"].as_str().unwrap_or_default().to_owned());
            }
            "Network.requestWillBeSent" => {
                let id = params["requestId"].as_str().unwrap_or_default().to_owned();
                // A redirect: the same request id, after the answer that
                // redirected it.
                if let Some(status) = params["redirectResponse"]["status"].as_u64()
                    && let Some(entry) = self.entry(&id)
                {
                    entry.status = u16::try_from(status).ok();
                }
                self.network_n += 1;
                let request = &params["request"];
                let entry = NetworkEntry {
                    n: self.network_n,
                    method: request["method"].as_str().unwrap_or("GET").to_owned(),
                    url: request["url"].as_str().unwrap_or_default().to_owned(),
                    status: None,
                    error: None,
                };
                push(&mut self.network, entry);
                self.requests.insert(id, self.network_n);
                if self.requests.len() > RING * 2 {
                    let oldest = self.network.front().map_or(0, |e| e.n);
                    self.requests.retain(|_, n| *n >= oldest);
                }
            }
            "Network.responseReceived" => {
                let id = params["requestId"].as_str().unwrap_or_default();
                let status = params["response"]["status"].as_u64();
                if let Some(entry) = self.entry(id) {
                    entry.status = status.and_then(|s| u16::try_from(s).ok());
                }
            }
            "Network.loadingFailed" => {
                let id = params["requestId"].as_str().unwrap_or_default();
                // `inspector`: Ovra held it, its origin was not allowed.
                let error = match params["blockedReason"].as_str() {
                    Some("inspector") => "not allowed".to_owned(),
                    Some(reason) => format!("blocked: {reason}"),
                    None => params["errorText"].as_str().unwrap_or("failed").to_owned(),
                };
                if let Some(entry) = self.entry(id) {
                    entry.status = None;
                    entry.error = Some(error);
                }
            }
            _ => {}
        }
    }

    pub(crate) fn console(&self, since: u64, limit: usize) -> Vec<ConsoleEntry> {
        after(&self.console, |e| e.n, since, limit)
    }

    pub(crate) fn network(&self, since: u64, limit: usize) -> Vec<NetworkEntry> {
        after(&self.network, |e| e.n, since, limit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn console_and_network_events_become_numbered_entries() {
        let mut logs = Logs::default();
        let events = [
            (
                "Runtime.consoleAPICalled",
                json!({"type": "log", "args": [
                    {"type": "string", "value": "count"}, {"type": "number", "value": 3},
                    {"type": "object", "description": "Object"}, {"type": "undefined"},
                ]}),
            ),
            (
                "Runtime.consoleAPICalled",
                json!({"type": "warning", "args": [{"type": "string", "value": "w"}]}),
            ),
            (
                "Log.entryAdded",
                json!({"entry": {"level": "error", "text": "Failed to load resource"}}),
            ),
            (
                "Runtime.exceptionThrown",
                json!({"exceptionDetails": {"text": "Uncaught", "exception": {"description": "TypeError: x is null"}}}),
            ),
            (
                "Network.requestWillBeSent",
                json!({"requestId": "1", "request": {"method": "GET", "url": "http://a/"}}),
            ),
            (
                "Network.requestWillBeSent",
                json!({"requestId": "1", "request": {"method": "GET", "url": "http://a/home"}, "redirectResponse": {"status": 302}}),
            ),
            (
                "Network.responseReceived",
                json!({"requestId": "1", "response": {"status": 200}}),
            ),
            (
                "Network.requestWillBeSent",
                json!({"requestId": "2", "request": {"method": "POST", "url": "https://x.example/api"}}),
            ),
            (
                "Network.loadingFailed",
                json!({"requestId": "2", "errorText": "net::ERR_TUNNEL_CONNECTION_FAILED"}),
            ),
            ("Page.loadEventFired", json!({})),
        ];
        for (method, params) in &events {
            logs.event(method, params);
        }
        let console: Vec<String> = logs
            .console(1, 10)
            .iter()
            .map(|e| format!("{} {} {}", e.n, e.level, e.text))
            .collect();
        assert_eq!(
            console,
            [
                "2 warning w",
                "3 error Failed to load resource",
                "4 error TypeError: x is null"
            ]
        );
        assert_eq!(logs.console(0, 1)[0].text, "count 3 Object undefined");
        let network: Vec<String> = logs
            .network(0, 10)
            .iter()
            .map(|e| {
                format!(
                    "{} {} {} {:?} {:?}",
                    e.n, e.method, e.url, e.status, e.error
                )
            })
            .collect();
        insta::assert_debug_snapshot!(network);
    }

    #[test]
    fn rings_keep_the_newest_entries() {
        let mut logs = Logs::default();
        for i in 0..RING + 5 {
            let args = json!({"type": "log", "args": [{"type": "number", "value": i}]});
            logs.event("Runtime.consoleAPICalled", &args);
        }
        let kept = logs.console(0, usize::MAX);
        assert_eq!((kept.len(), kept[0].n), (RING, 6));
    }
}
