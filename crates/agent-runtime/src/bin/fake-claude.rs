//! A stand-in `claude` that plays a recorded session (`fixtures/`), so e2e
//! tests run the real driver without the real CLI. Built only with the
//! `fake-claude` feature; never shipped.
//!
//! `OVRA_FAKE_CLAUDE_FIXTURE` names the fixture. `--version` and
//! `auth status` answer like Claude Code 2.1.331; anything else plays the
//! fixture: `cli` lines go to stdout, each `ovra` line must be the next line
//! on stdin, `wait`/`core`/`interrupt` steps are the test's part and skipped.
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
    let text = std::fs::read_to_string(&fixture).map_err(|e| format!("{fixture:?}: {e}"))?;
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let mut stdin = std::io::stdin().lock().lines();
    let mut stdout = std::io::stdout().lock();
    for (n, line) in text.lines().filter(|l| !l.trim().is_empty()).enumerate() {
        let step: Value = serde_json::from_str(line).map_err(|e| format!("step {}: {e}", n + 1))?;
        if let Some(line) = step.get("cli") {
            writeln!(stdout, "{}", localize(line.clone(), &cwd)).map_err(|e| e.to_string())?;
            stdout.flush().map_err(|e| e.to_string())?;
        } else if let Some(expected) = step.get("ovra") {
            let line = stdin
                .next()
                .ok_or_else(|| format!("step {}: stdin closed", n + 1))?
                .map_err(|e| e.to_string())?;
            let actual: Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
            let expected = localize(expected.clone(), &cwd);
            if actual != expected {
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
