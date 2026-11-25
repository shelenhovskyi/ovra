//! A stand-in language server for tests and e2e (feature `fake-server`).
//! It speaks the LSP base protocol on stdio: it answers `initialize`
//! (after asking the client for `workspace/configuration` once), reports a
//! diagnostic on each line with `TODO`, answers hover and definition, and
//! exits on `exit` or the end of its input. `fake/crash` makes it write to
//! stderr and exit with code 3.

use std::io::{BufRead, Write};

use serde_json::{Value, json};

fn main() -> std::io::Result<()> {
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let mut out = std::io::stdout().lock();
    while let Some(message) = read(&mut input)? {
        let method = message["method"].as_str().unwrap_or_default();
        let id = message.get("id").cloned();
        let params = &message["params"];
        let result = match method {
            "initialize" => {
                let ask = json!({
                    "jsonrpc": "2.0",
                    "id": "fake-configuration",
                    "method": "workspace/configuration",
                    "params": {"items": [{"section": "fake"}]},
                });
                write(&mut out, &ask)?;
                json!({"capabilities": {
                    "textDocumentSync": 1,
                    "hoverProvider": true,
                    "definitionProvider": true,
                }})
            }
            "textDocument/didOpen" => {
                let text = params["textDocument"]["text"].as_str();
                publish(&mut out, params, text)?;
                continue;
            }
            "textDocument/didChange" => {
                let text = params["contentChanges"]
                    .as_array()
                    .and_then(|changes| changes.last())
                    .and_then(|change| change["text"].as_str());
                publish(&mut out, params, text)?;
                continue;
            }
            "textDocument/hover" => {
                json!({"contents": {"kind": "markdown", "value": "fake hover"}})
            }
            "textDocument/definition" => json!({
                "uri": params["textDocument"]["uri"],
                "range": range(0, 0, 0),
            }),
            "shutdown" => Value::Null,
            "exit" => std::process::exit(0),
            "fake/crash" => {
                eprintln!("fake: crashed on purpose");
                std::process::exit(3);
            }
            _ => {
                // Notifications and answers to its own request need nothing.
                if let (Some(id), false) = (id, method.is_empty()) {
                    let error = json!({"code": -32601, "message": "not in the fake"});
                    write(
                        &mut out,
                        &json!({"jsonrpc": "2.0", "id": id, "error": error}),
                    )?;
                }
                continue;
            }
        };
        if let Some(id) = id {
            write(
                &mut out,
                &json!({"jsonrpc": "2.0", "id": id, "result": result}),
            )?;
        }
    }
    Ok(())
}

fn publish(out: &mut impl Write, params: &Value, text: Option<&str>) -> std::io::Result<()> {
    let diagnostics: Vec<Value> = text
        .unwrap_or_default()
        .lines()
        .enumerate()
        .filter_map(|(line, text)| {
            let at = text.find("TODO")?;
            Some(json!({
                "range": range(line, at, at + 4),
                "severity": 1,
                "source": "fake",
                "message": "fake: TODO found",
            }))
        })
        .collect();
    let notification = json!({
        "jsonrpc": "2.0",
        "method": "textDocument/publishDiagnostics",
        "params": {"uri": params["textDocument"]["uri"], "diagnostics": diagnostics},
    });
    write(out, &notification)
}

fn range(line: usize, start: usize, end: usize) -> Value {
    json!({
        "start": {"line": line, "character": start},
        "end": {"line": line, "character": end},
    })
}

/// The next message, `None` at the end of the input.
fn read(input: &mut impl BufRead) -> std::io::Result<Option<Value>> {
    let mut length = None;
    loop {
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.trim().eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse::<usize>().ok();
        }
    }
    let bad = |m: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, m.to_owned());
    let mut body = vec![0; length.ok_or_else(|| bad("no Content-Length"))?];
    input.read_exact(&mut body)?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|e| bad(&e.to_string()))
}

fn write(out: &mut impl Write, message: &Value) -> std::io::Result<()> {
    let body = message.to_string();
    write!(out, "Content-Length: {}\r\n\r\n{body}", body.len())?;
    out.flush()
}
