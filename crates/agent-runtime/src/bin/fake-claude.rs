//! A stand-in `claude` that plays a recorded session (`fixtures/`), so e2e
//! tests run the real driver without the real CLI. Built only with the
//! `fake-claude` feature; never shipped.
//!
//! `OVRA_FAKE_CLAUDE_FIXTURE` names the fixture, or a folder of them: a
//! session plays the one Ovra's lines match (its prompt, in practice). `--version` and
//! `auth status` answer like Claude Code 2.1.331; anything else plays the
//! fixture: `cli` lines go to stdout, each `ovra` line must be the next line
//! on stdin, `wait`/`core` steps are the test's part and skipped.
//! The recorded project `/w` becomes the working directory. Exits 2 when Ovra
//! writes something the recording did not.

use std::io::{BufRead, Write};
use std::path::Path;
use std::process::ExitCode;

use serde_json::Value;

const VERSION: &str = "2.1.331 (Claude Code)";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version") => {
            println!("{VERSION}");
            ExitCode::SUCCESS
        }
        Some("auth") => {
            println!(r#"{{"loggedIn":true}}"#);
            ExitCode::SUCCESS
        }
        _ => match play() {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("fake-claude: {message}");
                ExitCode::from(2)
            }
        },
    }
}

fn play() -> Result<(), String> {
    let fixture = std::env::var_os("OVRA_FAKE_CLAUDE_FIXTURE")
        .ok_or("OVRA_FAKE_CLAUDE_FIXTURE is not set")?;
    let mut fixtures = fixtures(Path::new(&fixture))?;
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let mut stdin = std::io::stdin().lock().lines();
    let mut stdout = std::io::stdout().lock();
    for n in 0.. {
        // The fixtures that match so far take the same step, until Ovra's
        // lines tell them apart.
        let steps: Vec<&Value> = fixtures.iter().filter_map(|f| f.steps.get(n)).collect();
        let Some(&step) = steps.first() else { break };
        if steps.len() < fixtures.len() || steps.iter().any(|s| kind(s) != kind(step)) {
            return Err(format!(
                "step {}: the fixtures differ before Ovra's lines tell them apart",
                n + 1
            ));
        }
        if let Some(line) = step.get("cli") {
            if steps.iter().any(|s| s.get("cli") != Some(line)) {
                return Err(format!(
                    "step {}: the fixtures differ before Ovra's lines tell them apart",
                    n + 1
                ));
            }
            let streamed = fixtures.iter().all(|f| f.streamed);
            for delta in deltas(line).into_iter().filter(|_| !streamed) {
                writeln!(stdout, "{delta}").map_err(|e| e.to_string())?;
            }
            writeln!(stdout, "{}", localize(line.clone(), &cwd)).map_err(|e| e.to_string())?;
            stdout.flush().map_err(|e| e.to_string())?;
        } else if step.get("ovra").is_some() {
            let line = stdin
                .next()
                .ok_or_else(|| format!("step {}: stdin closed", n + 1))?
                .map_err(|e| e.to_string())?;
            let actual: Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
            let expected = |f: &Fixture| f.steps[n].get("ovra").map(|v| localize(v.clone(), &cwd));
            let first = expected(&fixtures[0]);
            fixtures.retain(|f| expected(f).as_ref() == Some(&actual));
            if fixtures.is_empty() {
                let expected = first.unwrap_or_default();
                return Err(format!(
                    "step {}: Ovra wrote {actual}, expected {expected}",
                    n + 1
                ));
            }
        }
    }
    // Closing stdin ends the session.
    match stdin.next() {
        None => Ok(()),
        Some(line) => Err(format!(
            "Ovra wrote {} after the last step",
            line.map_err(|e| e.to_string())?
        )),
    }
}

/// A recorded session's steps.
struct Fixture {
    steps: Vec<Value>,
    /// Recorded with text deltas: it plays its own.
    streamed: bool,
}

/// The fixture at `path`, or every `.jsonl` fixture in the folder `path`.
fn fixtures(path: &Path) -> Result<Vec<Fixture>, String> {
    let files = if path.is_dir() {
        let mut files: Vec<_> = std::fs::read_dir(path)
            .map_err(|e| format!("{path:?}: {e}"))?
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|file| file.extension().is_some_and(|ext| ext == "jsonl"))
            .collect();
        files.sort();
        files
    } else {
        vec![path.to_path_buf()]
    };
    let fixtures = files
        .iter()
        .map(|file| {
            let text = std::fs::read_to_string(file).map_err(|e| format!("{file:?}: {e}"))?;
            let steps = text
                .lines()
                .filter(|l| !l.trim().is_empty())
                .enumerate()
                .map(|(n, line)| {
                    serde_json::from_str(line).map_err(|e| format!("{file:?} step {}: {e}", n + 1))
                })
                // `wait` and `core` steps are the test's part.
                .filter(|step| step.as_ref().map_or(true, |s| kind(s).is_some()))
                .collect::<Result<Vec<Value>, String>>()?;
            Ok(Fixture {
                streamed: text.contains(r#""type":"stream_event""#),
                steps,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    if fixtures.is_empty() {
        return Err(format!("{path:?} holds no fixture"));
    }
    Ok(fixtures)
}

/// `cli` or `ovra`: the steps this stand-in plays.
fn kind(step: &Value) -> Option<&'static str> {
    ["cli", "ovra"].into_iter().find(|k| step.get(k).is_some())
}

/// The `stream_event` text deltas `--include-partial-messages` writes before
/// an `assistant` message, a word each, for fixtures recorded without them.
fn deltas(line: &Value) -> Vec<Value> {
    if line["type"] != "assistant" {
        return Vec::new();
    }
    let texts = line["message"]["content"].as_array().into_iter().flatten();
    texts
        .filter(|block| block["type"] == "text")
        .filter_map(|block| block["text"].as_str())
        .flat_map(|text| text.split_inclusive(' '))
        .map(|word| {
            serde_json::json!({"type": "stream_event", "session_id": line["session_id"],
                "parent_tool_use_id": line["parent_tool_use_id"],
                "event": {"type": "content_block_delta", "index": 0,
                    "delta": {"type": "text_delta", "text": word}}})
        })
        .collect()
}

/// Replaces the recorded project `/w` with `cwd` in every string.
fn localize(value: Value, cwd: &Path) -> Value {
    match value {
        Value::String(s) => {
            let local = if s == "/w" {
                cwd.to_string_lossy().into_owned()
            } else if let Some(rest) = s.strip_prefix("/w/") {
                cwd.join(rest).to_string_lossy().into_owned()
            } else {
                s
            };
            Value::String(local)
        }
        Value::Array(items) => Value::Array(items.into_iter().map(|v| localize(v, cwd)).collect()),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(k, v)| (k, localize(v, cwd)))
                .collect(),
        ),
        other => other,
    }
}
