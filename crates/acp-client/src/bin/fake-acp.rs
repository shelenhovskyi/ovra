//! A stand-in ACP agent, so tests run the real adapter without a real
//! agent. Built only with the `fake-acp` feature; never shipped.
//!
//! It speaks ACP v1 on stdio and plays a script chosen by each prompt's
//! text:
//!
//! - `hello`: writes `Hello, world.` in two chunks.
//! - `run ls`: a command it asks about; allowed, it runs and says `Done.`,
//!   refused, it fails and says `Not allowed.`.
//! - `wait`: writes `Working...` and waits for `session/cancel`.
//! - `whoami`: writes its arguments, its folder and whether a sandbox
//!   marked its environment (`OVRA_FAKE_SANDBOX`).
//! - `crash`: writes to stderr and exits with 3.
//!
//! Exits when stdin closes; exits 2, saying why on stderr, when Ovra writes
//! what ACP v1 or the script does not allow (an `allow_always` answer).

use std::io::{BufRead, Write};
use std::process::ExitCode;

use serde_json::{Value, json};

const SESSION: &str = "fake-session";
/// The id of its permission request.
const ASK: u64 = 100;

fn main() -> ExitCode {
    match play() {
        Ok(()) => ExitCode::SUCCESS,
        Err(Exit::Crash) => {
            eprintln!("fake-acp: out of memory");
            ExitCode::from(3)
        }
        Err(Exit::Wrong(message)) => {
            eprintln!("fake-acp: {message}");
            ExitCode::from(2)
        }
    }
}

enum Exit {
    Crash,
    Wrong(String),
}

impl From<std::io::Error> for Exit {
    fn from(e: std::io::Error) -> Self {
        Self::Wrong(e.to_string())
    }
}

/// What it waits for in a turn.
enum Waiting {
    Answer { prompt: Value },
    Cancel { prompt: Value },
}

fn write(out: &mut impl Write, line: &Value) -> Result<(), Exit> {
    writeln!(out, "{line}")?;
    out.flush()?;
    Ok(())
}

fn respond(out: &mut impl Write, id: &Value, result: Value) -> Result<(), Exit> {
    write(out, &json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

fn update(out: &mut impl Write, update: Value) -> Result<(), Exit> {
    let params = json!({"sessionId": SESSION, "update": update});
    write(
        out,
        &json!({"jsonrpc": "2.0", "method": "session/update", "params": params}),
    )
}

fn say(out: &mut impl Write, text: &str) -> Result<(), Exit> {
    update(
        out,
        json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}}),
    )
}

fn end(out: &mut impl Write, prompt: &Value, reason: &str) -> Result<(), Exit> {
    respond(out, prompt, json!({"stopReason": reason}))
}

fn tool(out: &mut impl Write, status: &str, text: &str) -> Result<(), Exit> {
    update(
        out,
        json!({"sessionUpdate": "tool_call_update", "toolCallId": "call-1", "status": status,
            "content": [{"type": "content", "content": {"type": "text", "text": text}}]}),
    )
}

/// Starts the script of `text`; returns what the turn then waits for.
fn prompt(out: &mut impl Write, id: Value, text: &str) -> Result<Option<Waiting>, Exit> {
    match text {
        "hello" => {
            say(out, "Hello, ")?;
            say(out, "world.")?;
            end(out, &id, "end_turn")?;
        }
        "run ls" => {
            let call = json!({"toolCallId": "call-1", "title": "ls", "kind": "execute",
                "rawInput": {"command": "ls"}});
            let mut announced = call.clone();
            announced["sessionUpdate"] = json!("tool_call");
            announced["status"] = json!("pending");
            update(out, announced)?;
            let options = json!([
                {"optionId": "always", "name": "Always allow", "kind": "allow_always"},
                {"optionId": "once", "name": "Allow", "kind": "allow_once"},
                {"optionId": "no", "name": "Reject", "kind": "reject_once"},
            ]);
            let params = json!({"sessionId": SESSION, "toolCall": call, "options": options});
            write(
                out,
                &json!({"jsonrpc": "2.0", "id": ASK,
                    "method": "session/request_permission", "params": params}),
            )?;
            return Ok(Some(Waiting::Answer { prompt: id }));
        }
        "wait" => {
            say(out, "Working...")?;
            return Ok(Some(Waiting::Cancel { prompt: id }));
        }
        "whoami" => {
            let args: Vec<String> = std::env::args().skip(1).collect();
            let cwd = std::env::current_dir()?;
            let sandboxed = std::env::var_os("OVRA_FAKE_SANDBOX").is_some();
            let text = format!("args {args:?}, in {}, sandboxed {sandboxed}", cwd.display());
            say(out, &text)?;
            end(out, &id, "end_turn")?;
        }
        "crash" => return Err(Exit::Crash),
        other => return Err(Exit::Wrong(format!("no script for the prompt {other:?}"))),
    }
    Ok(None)
}

/// Goes on after the answer to its permission request.
fn answered(out: &mut impl Write, prompt: &Value, answer: &Value) -> Result<(), Exit> {
    let outcome = &answer["result"]["outcome"];
    match (outcome["outcome"].as_str(), outcome["optionId"].as_str()) {
        (Some("selected"), Some("once")) => {
            tool(out, "completed", "a.rs\nb.rs")?;
            say(out, "Done.")?;
            end(out, prompt, "end_turn")
        }
        (Some("selected"), Some("no")) => {
            tool(out, "failed", "The person refused.")?;
            say(out, "Not allowed.")?;
            end(out, prompt, "end_turn")
        }
        (Some("cancelled"), _) => end(out, prompt, "cancelled"),
        _ => Err(Exit::Wrong(format!("unexpected answer {answer}"))),
    }
}

fn play() -> Result<(), Exit> {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    let mut waiting = None;
    for line in stdin.lock().lines() {
        let message: Value =
            serde_json::from_str(&line?).map_err(|e| Exit::Wrong(e.to_string()))?;
        let id = message["id"].clone();
        let params = &message["params"];
        match (message["method"].as_str(), waiting.take()) {
            (Some("initialize"), None) if params["protocolVersion"] == 1 => {
                respond(
                    &mut out,
                    &id,
                    json!({"protocolVersion": 1, "agentCapabilities": {}}),
                )?;
            }
            (Some("session/new"), None) if params["mcpServers"] == json!([]) => {
                respond(&mut out, &id, json!({"sessionId": SESSION}))?;
            }
            (Some("session/prompt"), None) => {
                let text = params["prompt"][0]["text"].as_str().unwrap_or_default();
                waiting = prompt(&mut out, id, text)?;
            }
            (Some("session/cancel"), Some(Waiting::Cancel { prompt })) => {
                end(&mut out, &prompt, "cancelled")?;
            }
            // The turn ended as Ovra cancelled it.
            (Some("session/cancel"), None) => {}
            // Ovra cancels, then answers its request with `cancelled`.
            (Some("session/cancel"), Some(asking @ Waiting::Answer { .. })) => {
                waiting = Some(asking);
            }
            (None, Some(Waiting::Answer { prompt })) if id == ASK => {
                answered(&mut out, &prompt, &message)?;
            }
            _ => return Err(Exit::Wrong(format!("unexpected {message}"))),
        }
    }
    Ok(())
}
