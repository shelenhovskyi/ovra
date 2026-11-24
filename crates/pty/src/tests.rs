use std::path::{Path, PathBuf};
use std::sync::Arc;

use ovra_core::Core;
use ovra_core::protocol::terminals::{TermSize, TerminalId, TerminalsEvent};
use ovra_core::protocol::{ErrorCode, Event, Request, Response};
use ovra_core::services::TerminalService;
use tokio::sync::broadcast::Receiver;

use super::Pty;

const SIZE: TermSize = TermSize { cols: 80, rows: 24 };

fn here() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .canonicalize()
        .unwrap_or_else(|e| panic!("{e}"))
}

/// The next `terminals.*` event for `id`.
async fn next(rx: &mut Receiver<Event>, id: &TerminalId) -> TerminalsEvent {
    loop {
        match rx.recv().await {
            Ok(Event::Terminals(e)) => {
                let (TerminalsEvent::Output { id: of, .. } | TerminalsEvent::Exited { id: of, .. }) =
                    &e;
                if of == id {
                    return e;
                }
            }
            Ok(_) => {}
            Err(e) => panic!("{e}"),
        }
    }
}

/// Collects output until it contains `needle`; returns all of it.
async fn output_until(rx: &mut Receiver<Event>, id: &TerminalId, needle: &str) -> String {
    let mut text = String::new();
    while !text.contains(needle) {
        match next(rx, id).await {
            TerminalsEvent::Output { data, .. } => text += &data,
            exited => panic!("{exited:?} before {needle:?} in {text:?}"),
        }
    }
    text
}

/// Skips output until the terminal exits; returns the exit code.
async fn exit_code(rx: &mut Receiver<Event>, id: &TerminalId) -> Option<i32> {
    loop {
        if let TerminalsEvent::Exited { code, .. } = next(rx, id).await {
            return code;
        }
    }
}

#[cfg(unix)]
fn shell() -> Pty {
    Pty::with_program(&["/bin/sh"])
}

#[tokio::test]
async fn runs_a_program_streams_its_output_and_reports_the_exit_code() {
    #[cfg(unix)]
    let pty = Pty::with_program(&["/bin/sh", "-c", "printf 'h%s\\n' i; exit 3"]);
    #[cfg(windows)]
    let pty = Pty::with_program(&["cmd.exe", "/c", "echo hi& exit 3"]);
    let core = Core::new("test");
    let mut rx = core.subscribe();
    let id = pty.open(here(), SIZE, core.sink()).expect("open");
    output_until(&mut rx, &id, "hi").await;
    assert_eq!(exit_code(&mut rx, &id).await, Some(3));
    let gone = pty.input(&id, "x").expect_err("closed");
    assert_eq!(gone.code, ErrorCode::NotFound);
}

#[cfg(unix)]
#[tokio::test]
async fn shell_starts_in_the_project_and_follows_resizes() {
    let pty = shell();
    let core = Core::new("test");
    let mut rx = core.subscribe();
    let id = pty.open(here(), SIZE, core.sink()).expect("open");

    // Markers are split so the echoed command line does not match them.
    pty.input(&id, "echo \"<$(pwd)>\"\r").expect("input");
    output_until(&mut rx, &id, &format!("<{}>", here().display())).await;

    pty.resize(
        &id,
        TermSize {
            cols: 100,
            rows: 30,
        },
    )
    .expect("resize");
    pty.input(&id, "echo \"[$(stty size)]\"\r").expect("input");
    output_until(&mut rx, &id, "[30 100]").await;

    pty.input(&id, "exit 0\r").expect("input");
    assert_eq!(exit_code(&mut rx, &id).await, Some(0));
}

#[cfg(unix)]
#[tokio::test]
async fn keeps_multibyte_text_whole() {
    let pty = shell();
    let core = Core::new("test");
    let mut rx = core.subscribe();
    let id = pty.open(here(), SIZE, core.sink()).expect("open");
    // Octal escapes keep the echoed command line ASCII.
    pty.input(&id, "printf '<\\303\\251\\342\\202\\254>\\n'\r")
        .expect("input");
    let text = output_until(&mut rx, &id, "<é€>").await;
    assert!(!text.contains('\u{fffd}'), "{text:?}");
    pty.close(&id).expect("close");
    exit_code(&mut rx, &id).await;
}

#[cfg(unix)]
#[tokio::test]
async fn close_ends_the_shell_and_forgets_the_terminal() {
    let pty = shell();
    let core = Core::new("test");
    let mut rx = core.subscribe();
    let first = pty.open(here(), SIZE, core.sink()).expect("open");
    let second = pty.open(here(), SIZE, core.sink()).expect("open");
    assert_eq!((first.0.as_str(), second.0.as_str()), ("t1", "t2"));

    pty.close(&first).expect("close");
    // Killed by SIGHUP, so no exit code.
    assert_eq!(exit_code(&mut rx, &first).await, None);
    let gone = pty.close(&first).expect_err("closed");
    assert_eq!(gone.code, ErrorCode::NotFound);

    // The other terminal is untouched.
    pty.input(&second, "echo \"<$((1+1))>\"\r").expect("input");
    output_until(&mut rx, &second, "<2>").await;
    pty.close(&second).expect("close");
    exit_code(&mut rx, &second).await;
}

#[test]
fn refuses_a_missing_folder_and_unknown_ids() {
    let pty = Pty::new();
    let core = Core::new("test");
    let missing = here().join("no-such-folder");
    let err = pty.open(missing, SIZE, core.sink()).expect_err("missing");
    assert_eq!(err.code, ErrorCode::NotFound);
    let unknown = TerminalId("t9".into());
    let codes = [
        pty.input(&unknown, "x").err().map(|e| e.code),
        pty.resize(&unknown, SIZE).err().map(|e| e.code),
        pty.close(&unknown).err().map(|e| e.code),
    ];
    assert_eq!(codes, [Some(ErrorCode::NotFound); 3]);
}

#[cfg(unix)]
#[tokio::test]
async fn opens_through_the_core_in_the_project_root() {
    let core = Core::builder("test")
        .projects(Arc::new(ovra_core::fakes::MemoryProjects::default()))
        .terminals(Arc::new(shell()))
        .build();
    let mut rx = core.subscribe();
    let root = here();
    let add = serde_json::json!({"method": "projects.add", "params": {"root": root}});
    let open = serde_json::json!({"method": "terminals.open",
        "params": {"project": "p1", "size": {"cols": 80, "rows": 24}}});
    let mut responses = Vec::new();
    for json in [add, open] {
        let request: Request = serde_json::from_value(json).unwrap_or_else(|e| panic!("{e}"));
        responses.push(
            core.handle(request)
                .await
                .unwrap_or_else(|e| panic!("{e:?}")),
        );
    }
    let Some(Response::Terminals(opened)) = responses.pop() else {
        panic!("{responses:?}");
    };
    let id = serde_json::to_value(&opened).unwrap_or_else(|e| panic!("{e}"))["data"]["id"]
        .as_str()
        .map(|s| TerminalId(s.to_owned()))
        .unwrap_or_else(|| panic!("{opened:?}"));

    let input = serde_json::json!({"method": "terminals.input",
        "params": {"id": id, "data": "echo \"<$(pwd)>\"; exit 5\r"}});
    let request: Request = serde_json::from_value(input).unwrap_or_else(|e| panic!("{e}"));
    core.handle(request)
        .await
        .unwrap_or_else(|e| panic!("{e:?}"));
    output_until(&mut rx, &id, &format!("<{}>", root.display())).await;
    assert_eq!(exit_code(&mut rx, &id).await, Some(5));
}
