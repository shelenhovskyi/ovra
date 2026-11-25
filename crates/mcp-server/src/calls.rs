//! The tools themselves: arguments in, the core's answer out as the text an
//! agent reads. Arguments follow `tools.json`; unknown fields are refused.

use ovra_core::Core;
use ovra_core::protocol::ProtocolError;
use ovra_core::protocol::agents::SessionId;
use ovra_core::protocol::orchestration::{ShowTarget, Worker};
use ovra_core::protocol::review::ReviewId;
use ovra_core::services::ToolCaller;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

/// `read_output` returns this many messages unless asked for another number.
const DEFAULT_MESSAGES: usize = 50;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Nothing {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Spawn {
    task: String,
    name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Agent {
    agent_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadOutput {
    agent_id: String,
    #[serde(default)]
    since: usize,
    #[serde(default = "default_messages")]
    max_messages: usize,
}

fn default_messages() -> usize {
    DEFAULT_MESSAGES
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Send {
    agent_id: String,
    text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestReview {
    summary: String,
    agent_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Open {
    path: Option<String>,
    line: Option<u32>,
    agent_id: Option<String>,
    review_id: Option<String>,
}

/// Runs tool `name` for `caller`; `Err` is a tool error the agent reads.
pub(crate) async fn call(
    core: &Core,
    caller: &ToolCaller,
    name: &str,
    arguments: Value,
) -> Result<String, String> {
    match name {
        "spawn_agent" => {
            let Spawn { task, name } = parse(arguments)?;
            let worker = core
                .spawn_agent(caller, &name, &task)
                .await
                .map_err(failed)?;
            Ok(text(&worker_json(&worker)))
        }
        "list_agents" => {
            let Nothing {} = parse(arguments)?;
            let agents: Vec<Value> = core
                .list_agents(caller)
                .map_err(failed)?
                .into_iter()
                .map(|state| {
                    let mut agent = worker_json(&state.worker);
                    agent["status"] = json!(state.status);
                    agent["review"] = state
                        .review
                        .map_or(Value::Null, |(id, state)| json!({"id": id, "state": state}));
                    agent
                })
                .collect();
            Ok(text(&json!({ "agents": agents })))
        }
        "read_output" => {
            let args: ReadOutput = parse(arguments)?;
            let output = core
                .read_output(
                    caller,
                    &SessionId(args.agent_id),
                    args.since,
                    args.max_messages,
                )
                .map_err(failed)?;
            Ok(text(
                &json!({"messages": output.messages, "next": output.next}),
            ))
        }
        "send_message" => {
            let Send { agent_id, text } = parse(arguments)?;
            let agent = SessionId(agent_id);
            core.send_message(caller, &agent, &text).map_err(failed)?;
            Ok(format!("Sent to {}.", agent.0))
        }
        "request_review" => {
            let RequestReview { summary, agent_id } = parse(arguments)?;
            let agent = agent_id.map(SessionId);
            let item = core
                .request_review(caller, agent.as_ref(), &summary)
                .await
                .map_err(failed)?;
            let files: Vec<&str> = item.files.iter().map(|f| f.path.as_str()).collect();
            Ok(text(
                &json!({"review_id": item.id, "state": item.state, "files": files}),
            ))
        }
        "open_in_ui" => {
            let target = target(parse(arguments)?)?;
            core.open_in_ui(caller, target).map_err(failed)?;
            Ok("Shown in Ovra.".to_owned())
        }
        "stop_agent" => {
            let Agent { agent_id } = parse(arguments)?;
            let agent = SessionId(agent_id);
            core.stop_agent(caller, &agent).map_err(failed)?;
            Ok(format!("Stopped {}.", agent.0))
        }
        _ => crate::servers::call(core, caller, name, arguments)
            .await
            .unwrap_or_else(|| Err(format!("unknown tool: {name}"))),
    }
}

/// The one thing `open_in_ui` names.
fn target(open: Open) -> Result<ShowTarget, String> {
    match open {
        Open {
            path: Some(path),
            line,
            agent_id: None,
            review_id: None,
        } => Ok(ShowTarget::File { path, line }),
        Open {
            path: None,
            line: None,
            agent_id: Some(agent),
            review_id: None,
        } => Ok(ShowTarget::Agent {
            session: SessionId(agent),
        }),
        Open {
            path: None,
            line: None,
            agent_id: None,
            review_id: Some(id),
        } => Ok(ShowTarget::Review { id: ReviewId(id) }),
        _ => Err(
            "name exactly one of path (with an optional line), agent_id or review_id".to_owned(),
        ),
    }
}

fn worker_json(worker: &Worker) -> Value {
    json!({
        "agent_id": worker.session,
        "name": worker.name,
        "branch": worker.branch,
        "worktree": worker.worktree,
    })
}

/// Missing arguments are an empty object, as for a tool without parameters.
pub(crate) fn parse<T: DeserializeOwned>(arguments: Value) -> Result<T, String> {
    let arguments = if arguments.is_null() {
        json!({})
    } else {
        arguments
    };
    serde_json::from_value(arguments).map_err(|e| format!("invalid arguments: {e}"))
}

pub(crate) fn failed(error: ProtocolError) -> String {
    error.to_string()
}

pub(crate) fn text(value: &Value) -> String {
    value.to_string()
}
