//! A stand-in language server for tests and e2e (feature `fake-server`).
//! It speaks the LSP base protocol on stdio: it answers `initialize`
//! (after asking the client for `workspace/configuration` once), reports a
//! diagnostic on each line with `TODO`, answers hover, definition,
//! completion, and references and rename by the word at the position (in
//! open files and the `.rs` files at the root), offers a quick fix that
//! replaces each `TODO` diagnostic given with `DONE`, and exits on `exit` or the
//! end of its input. `fake/crash` makes it write to stderr and exit with
//! code 3.

use std::collections::BTreeMap;
use std::io::{BufRead, Write};

use serde_json::{Value, json};

fn main() -> std::io::Result<()> {
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let mut out = std::io::stdout().lock();
    let mut docs: BTreeMap<String, String> = BTreeMap::new();
    let mut root: Option<String> = None;
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
                root = params["rootUri"].as_str().map(str::to_owned);
                json!({"capabilities": {
                    "textDocumentSync": 1,
                    "hoverProvider": true,
                    "definitionProvider": true,
                    "referencesProvider": true,
                    "renameProvider": true,
                    "completionProvider": {},
                    "codeActionProvider": true,
                }})
            }
            "textDocument/didOpen" => {
                let text = params["textDocument"]["text"].as_str();
                if let (Some(uri), Some(text)) = (params["textDocument"]["uri"].as_str(), text) {
                    docs.insert(uri.to_owned(), text.to_owned());
                }
                publish(&mut out, params, text)?;
                continue;
            }
            "textDocument/didClose" => {
                if let Some(uri) = params["textDocument"]["uri"].as_str() {
                    docs.remove(uri);
                }
                continue;
            }
            "textDocument/didChange" => {
                let text = params["contentChanges"]
                    .as_array()
                    .and_then(|changes| changes.last())
                    .and_then(|change| change["text"].as_str());
                if let (Some(uri), Some(text)) = (params["textDocument"]["uri"].as_str(), text) {
                    docs.insert(uri.to_owned(), text.to_owned());
                }
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
            "textDocument/completion" => json!({
                "isIncomplete": false,
                "items": [{"label": "fake_item", "detail": "from the fake"}],
            }),
            "textDocument/references" => {
                let found = occurrences(&docs, root.as_deref(), params);
                let locations: Vec<Value> = found
                    .into_iter()
                    .flat_map(|(uri, ranges)| {
                        ranges
                            .into_iter()
                            .map(move |range| json!({"uri": uri, "range": range}))
                    })
                    .collect();
                json!(locations)
            }
            "textDocument/rename" => {
                let name = params["newName"].as_str().unwrap_or_default();
                let changes: serde_json::Map<String, Value> =
                    occurrences(&docs, root.as_deref(), params)
                        .into_iter()
                        .map(|(uri, ranges)| {
                            let edits: Vec<Value> = ranges
                                .into_iter()
                                .map(|range| json!({"range": range, "newText": name}))
                                .collect();
                            (uri, json!(edits))
                        })
                        .collect();
                json!({"changes": changes})
            }
            "textDocument/codeAction" => {
                let uri = &params["textDocument"]["uri"];
                let fixes: Vec<Value> = params["context"]["diagnostics"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|d| d["message"] == "fake: TODO found")
                    .map(|d| {
                        let edit = json!({"range": d["range"], "newText": "DONE"});
                        let mut changes = serde_json::Map::new();
                        changes.insert(uri.as_str().unwrap_or_default().to_owned(), json!([edit]));
                        json!({
                            "title": "Replace TODO with DONE",
                            "kind": "quickfix",
                            "isPreferred": true,
                            "edit": {"changes": changes},
                        })
                    })
                    .collect();
                json!(fixes)
            }
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

/// The word at the request's position, found in every open file and in
/// the `.rs` files at the root that are not open, by URI.
fn occurrences(
    docs: &BTreeMap<String, String>,
    root: Option<&str>,
    params: &Value,
) -> BTreeMap<String, Vec<Value>> {
    let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
    let line = params["position"]["line"].as_u64().unwrap_or(0) as usize;
    let at = params["position"]["character"].as_u64().unwrap_or(0) as usize;
    let word = docs
        .get(uri)
        .and_then(|text| text.lines().nth(line))
        .and_then(|text| word_at(text, at));
    let Some(word) = word else {
        return BTreeMap::new();
    };
    let mut files: BTreeMap<String, String> = docs.clone();
    // Plain file names only, so the URI needs no encoding.
    let plain = |name: &str| {
        name.ends_with(".rs")
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
    };
    if let Some(root) = root
        && let Some(dir) = root.strip_prefix("file://")
        && let Ok(entries) = std::fs::read_dir(dir)
    {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let uri = format!("{}/{name}", root.trim_end_matches('/'));
            if plain(&name)
                && !files.contains_key(&uri)
                && let Ok(text) = std::fs::read_to_string(entry.path())
            {
                files.insert(uri, text);
            }
        }
    }
    let word = word.as_str();
    files
        .into_iter()
        .filter_map(|(uri, text)| {
            let mut ranges = Vec::new();
            for (n, line) in text.lines().enumerate() {
                for start in whole_words(line, word) {
                    ranges.push(range(n, start, start + word.len()));
                }
            }
            (!ranges.is_empty()).then_some((uri, ranges))
        })
        .collect()
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// The ASCII word around byte `at` of `line`.
fn word_at(line: &str, at: usize) -> Option<String> {
    let bytes = line.as_bytes();
    let at = at.min(bytes.len());
    let start = line[..at].rfind(|c: char| !is_word(c)).map_or(0, |i| i + 1);
    let end = line[at..]
        .find(|c: char| !is_word(c))
        .map_or(line.len(), |i| at + i);
    (start < end).then(|| line[start..end].to_owned())
}

/// Where `word` starts in `line` as a whole word.
fn whole_words<'a>(line: &'a str, word: &'a str) -> impl Iterator<Item = usize> + 'a {
    line.match_indices(word).filter_map(move |(start, _)| {
        let before = line[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !is_word(c));
        let after = line[start + word.len()..]
            .chars()
            .next()
            .is_none_or(|c| !is_word(c));
        (before && after).then_some(start)
    })
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
