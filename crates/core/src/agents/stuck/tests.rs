use serde_json::json;

use super::*;
use crate::protocol::agents::{MessageFrom, ToolKind};

fn call(id: &str, name: &str, command: &str) -> AgentMessage {
    AgentMessage::ToolUse {
        id: id.into(),
        name: name.into(),
        input: json!({"command": command}),
        call: ToolCall {
            kind: ToolKind::Execute,
            command: Some(command.into()),
            ..ToolCall::default()
        },
        parent: None,
    }
}

fn result(id: &str, output: &str, is_error: bool) -> AgentMessage {
    AgentMessage::ToolResult {
        tool_use_id: id.into(),
        output: output.into(),
        is_error,
        parent: None,
    }
}

/// Runs `command` once with `output`.
fn run(watch: &mut Watch, n: u32, command: &str, output: &str, is_error: bool) {
    let id = format!("t{n}");
    watch.see(&call(&id, "Bash", command));
    watch.see(&result(&id, output, is_error));
}

#[tokio::test]
async fn the_same_call_with_the_same_result_three_times_is_a_loop() {
    let mut watch = Watch::default();
    run(&mut watch, 1, "cargo test", "1 failed", true);
    run(&mut watch, 2, "cargo test", "1 failed", true);
    assert_eq!(watch.hint(), None);
    run(&mut watch, 3, "cargo test", "1 failed", true);
    insta::assert_json_snapshot!(watch.hint(), @r#"
    {
      "kind": "repeating",
      "name": "Bash",
      "call": {
        "kind": "execute",
        "command": "cargo test"
      },
      "times": 3
    }
    "#);
    // Told once; a new turn takes the hint along.
    assert!(watch.changed().is_some());
    assert_eq!(watch.changed(), None);
    watch.see(&AgentMessage::UserText {
        text: "try another way".into(),
        from: MessageFrom::Person,
    });
    assert_eq!(watch.changed(), Some(None));
}

#[tokio::test]
async fn a_rerun_that_gets_a_new_result_is_progress() {
    let mut watch = Watch::default();
    for (n, output) in ["3 failed", "2 failed", "1 failed", "ok"]
        .iter()
        .enumerate()
    {
        run(&mut watch, n as u32, "cargo test", output, false);
    }
    assert_eq!(watch.hint(), None);
}

#[tokio::test]
async fn failures_in_a_row_until_one_works() {
    let mut watch = Watch::default();
    for n in 0..5 {
        run(&mut watch, n, &format!("try {n}"), "not found", true);
    }
    assert_eq!(watch.hint(), Some(Stuck::Failing { times: 5 }));
    run(&mut watch, 9, "ls", "a", false);
    assert_eq!(watch.hint(), None);
}

#[tokio::test]
async fn ovra_tools_subagents_and_old_calls_do_not_count() {
    let mut watch = Watch::default();
    // A coordinator that asks about its workers while it waits.
    for n in 0..4 {
        let id = format!("w{n}");
        watch.see(&call(&id, "mcp__ovra__list_agents", ""));
        watch.see(&result(&id, "working", false));
    }
    // Subagents that each run the same thing.
    for n in 0..4 {
        let id = format!("s{n}");
        let AgentMessage::ToolUse {
            id: i,
            name,
            input,
            call: c,
            ..
        } = call(&id, "Bash", "git status")
        else {
            unreachable!()
        };
        watch.see(&AgentMessage::ToolUse {
            id: i,
            name,
            input,
            call: c,
            parent: Some("task".into()),
        });
        watch.see(&AgentMessage::ToolResult {
            tool_use_id: id,
            output: "clean".into(),
            is_error: false,
            parent: Some("task".into()),
        });
    }
    assert_eq!(watch.hint(), None);
    // Three repeats that slid out of the window of recent calls.
    for n in 0..3 {
        run(&mut watch, n, "make", "same", false);
    }
    assert!(watch.hint().is_some());
    for n in 10..(10 + WINDOW as u32) {
        run(&mut watch, n, &format!("step {n}"), "ok", false);
    }
    assert_eq!(watch.hint(), None);
}

#[tokio::test(start_paused = true)]
async fn silence_while_working_until_the_agent_shows_something() {
    let mut watch = Watch::default();
    watch.status(&AgentStatus::Working);
    let now = Instant::now();
    assert_eq!(watch.silence(now), Some(now + SILENT));
    assert_eq!(watch.silence(now + SILENT), None);
    assert_eq!(watch.hint(), Some(Stuck::Silent { minutes: 10 }));
    watch.active();
    assert_eq!(watch.hint(), None);
    // A turn that ends clears what it repeated, too.
    for n in 0..3 {
        run(&mut watch, n, "make", "same", false);
    }
    watch.status(&AgentStatus::Idle);
    assert_eq!(watch.hint(), None);
}
