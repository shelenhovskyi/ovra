//! The `server_*` methods agents reach through the IDE tools.

use std::time::Duration;

use super::{Fx, caller, code, coordinator_and_worker, fx, list, person_starts};
use crate::protocol::ErrorCode;
use crate::protocol::servers::{ServerId, ServerStatus};
use crate::services::ToolRole;

fn srv(id: &str) -> ServerId {
    ServerId(id.into())
}

#[tokio::test]
async fn an_agent_reaches_only_its_own_work_dirs_servers() {
    let f: Fx = fx().await;
    coordinator_and_worker(&f).await;
    let boss = caller("s1", ToolRole::Coordinator);
    let worker = caller("s2", ToolRole::Worker);
    let mine = f.core.server_list(&boss).await;
    assert_eq!(mine.ok(), Some(list(&f, None).await));
    let theirs = f.core.server_list(&worker).await;
    assert_eq!(theirs.ok(), Some(list(&f, Some("s2")).await));
    let web = person_starts(&f, "web").await;
    let own = f.core.server_start(&worker, "web", Duration::ZERO).await;
    let own = own.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        code(f.core.server_start(&worker, "api", Duration::ZERO).await),
        ErrorCode::NotFound
    );
    // Neither reaches the other's servers by id.
    assert_eq!(
        code(f.core.server_stop(&worker, &web.id).await),
        ErrorCode::NotFound
    );
    assert_eq!(
        code(f.core.server_logs(&boss, &own.id, 0, 10, None)),
        ErrorCode::NotFound
    );
    assert_eq!(
        code(f.core.server_start(&worker, "web", Duration::ZERO).await),
        ErrorCode::Conflict
    );

    f.dev.output(
        &web.id,
        &[
            "\u{1b}[1mstarting\u{1b}[0m",
            "error: one",
            "ok",
            "error: two",
        ],
    );
    let errors = f.core.server_logs(&boss, &web.id, 0, 10, Some("error"));
    let errors = errors.unwrap_or_else(|e| panic!("{e}"));
    let texts: Vec<&str> = errors.lines.iter().map(|l| l.text.as_str()).collect();
    assert_eq!((texts, errors.next), (vec!["error: one", "error: two"], 4));
    let plain = f
        .core
        .server_logs(&boss, &web.id, 0, 1, None)
        .map(|p| p.lines[0].text.clone());
    assert_eq!(plain.ok().as_deref(), Some("starting"));
}

#[tokio::test(start_paused = true)]
async fn an_agents_start_waits_until_ready_or_the_wait_is_over() {
    let f: Fx = fx().await;
    coordinator_and_worker(&f).await;
    let boss = caller("s1", ToolRole::Coordinator);
    let dev = f.dev.clone();
    let ready = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(1)).await;
        dev.report(&srv("srv1"), ServerStatus::Ready)
    });
    let web = f
        .core
        .server_start(&boss, "web", Duration::from_secs(30))
        .await;
    assert_eq!(web.map(|w| w.status).ok(), Some(ServerStatus::Ready));
    assert!(ready.await.unwrap_or_default());
    let api = f
        .core
        .server_start(&boss, "api", Duration::from_secs(5))
        .await;
    assert_eq!(api.map(|a| a.status).ok(), Some(ServerStatus::Starting));
    let stopped = f.core.server_stop(&boss, &srv("srv2")).await;
    assert_eq!(
        stopped.map(|a| a.status).ok(),
        Some(ServerStatus::Exited { code: None })
    );
    let restarted = f
        .core
        .server_restart(&boss, &srv("srv1"), Duration::ZERO)
        .await;
    assert_eq!(
        restarted.map(|a| (a.generation, a.status)).ok(),
        Some((2, ServerStatus::Starting))
    );
}
