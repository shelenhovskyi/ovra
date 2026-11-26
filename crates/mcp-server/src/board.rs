//! The task board's tools (`docs/specs/task-board.md`): every answer that
//! carries agent-written text is escaped, so it cannot pass for a mark of
//! the core's.

use ovra_core::Core;
use ovra_core::protocol::board::EntryId;
use ovra_core::provenance::escaped;
use ovra_core::services::ToolCaller;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::calls::{failed, parse, text};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Claim {
    paths: Vec<String>,
    #[serde(default)]
    note: String,
    worker: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Release {
    paths: Vec<String>,
    worker: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ask {
    to: String,
    question: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Answer {
    question_id: String,
    text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Post {
    text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Read {
    #[serde(default)]
    since: usize,
}

/// Runs board tool `name`; `None` if it is not one.
pub(crate) fn call(
    core: &Core,
    caller: &ToolCaller,
    name: &str,
    arguments: Value,
) -> Option<Result<String, String>> {
    let answer = match name {
        "claim" => parse(arguments).and_then(|a: Claim| {
            let claimed = core
                .board_claim(caller, a.worker.as_deref(), &a.paths, &a.note)
                .map_err(failed)?;
            let overlaps: Vec<Value> = claimed
                .overlaps
                .iter()
                .map(|(worker, path)| json!({"worker": worker, "path": path}))
                .collect();
            Ok(json!({"entry": claimed.entry, "overlaps": overlaps}))
        }),
        "release" => parse(arguments).and_then(|a: Release| {
            let entry = core
                .board_release(caller, a.worker.as_deref(), &a.paths)
                .map_err(failed)?;
            Ok(json!({ "entry": entry }))
        }),
        "ask" => parse(arguments).and_then(|a: Ask| {
            let (entry, later) = core.board_ask(caller, &a.to, &a.question).map_err(failed)?;
            Ok(json!({"question_id": entry.id, "note": later}))
        }),
        "answer" => parse(arguments).and_then(|a: Answer| {
            let entry = core
                .board_answer(caller, &EntryId(a.question_id), &a.text)
                .map_err(failed)?;
            Ok(json!({ "entry": entry }))
        }),
        "post" => parse(arguments).and_then(|a: Post| {
            let entry = core.board_post(caller, &a.text).map_err(failed)?;
            Ok(json!({ "entry": entry }))
        }),
        "read_board" => parse(arguments).and_then(|a: Read| {
            let (entries, next) = core.board_read(caller, a.since).map_err(failed)?;
            Ok(json!({"entries": entries, "next": next}))
        }),
        _ => return None,
    };
    Some(answer.map(|value| escaped(&text(&value))))
}
