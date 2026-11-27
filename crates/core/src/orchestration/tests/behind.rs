//! The project moved on under a worker (`docs/specs/main-moved.md`).

use super::*;
use crate::protocol::review::Behind;

/// A task whose worker `s2` changed `a.rs` and `b.rs` and is idle; returns
/// its worktree.
async fn worker(f: &Fx, rx: &mut broadcast::Receiver<Event>) -> String {
    f.wt.put(ROOT, "b.rs", "b\n");
    start_task(&f.core, 3).await;
    let boss = caller("s1", ToolRole::Coordinator);
    let worker = f.core.spawn_agent(&boss, "w", "x", false).await;
    let worker = worker.unwrap_or_else(|e| panic!("{e}"));
    until(rx, "s2", AgentStatus::Idle).await;
    f.wt.put(&worker.worktree, "a.rs", "one\nworker\n");
    f.wt.put(&worker.worktree, "b.rs", "worker b\n");
    worker.worktree
}

async fn update(f: &Fx, conflicts: bool) -> (ReviewItem, u32, Vec<String>, bool) {
    let done = ok(
        &f.core,
        json!({"method": "review.update", "params": {"id": "r1", "conflicts": conflicts}}),
    )
    .await;
    let Response::Review(ReviewResponse::Updated {
        item,
        files,
        conflicts,
        written,
    }) = done
    else {
        panic!("unexpected {done:?}");
    };
    (item, files, conflicts, written)
}

#[tokio::test]
async fn the_inbox_learns_which_files_keep_would_leave_out() {
    let f = fx(say()).await;
    let mut rx = f.core.subscribe();
    worker(&f, &mut rx).await;
    assert_eq!(reviews(&f.core).await[0].behind, None, "nothing moved yet");

    // The person edits a file of the worker's and one of their own.
    f.wt.put(ROOT, "b.rs", "mine\n");
    f.wt.put(ROOT, "c.rs", "new\n");
    let item = reviews(&f.core).await.remove(0);
    assert_eq!(
        item.behind,
        Some(Behind {
            files: 2,
            clashes: vec!["b.rs".to_owned()]
        })
    );
}

#[tokio::test]
async fn an_update_brings_the_project_in_and_keep_then_copies_everything() {
    let f = fx(say()).await;
    let mut rx = f.core.subscribe();
    let dir = worker(&f, &mut rx).await;
    f.wt.put(ROOT, "c.rs", "new\n");

    let (item, files, conflicts, written) = update(&f, false).await;
    assert_eq!((files, conflicts.len(), written), (1, 0, true));
    assert_eq!(item.behind, None);
    let paths: Vec<&str> = item.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["a.rs", "b.rs"], "only the worker's own changes");
    assert_eq!(
        f.wt.files(&dir).get("c.rs").map(String::as_str),
        Some("new\n")
    );

    ok(&f.core, approval(&f.core, "r1", json!(null)).await).await;
    let main = f.wt.files(ROOT);
    assert_eq!(main.get("b.rs").map(String::as_str), Some("worker b\n"));
    assert_eq!(main.get("c.rs").map(String::as_str), Some("new\n"));
    assert_eq!(reviews(&f.core).await[0].state, ReviewState::Approved);
}

#[tokio::test]
async fn conflicts_are_written_only_when_the_person_asks_and_the_agent_hears_of_them() {
    let f = fx(say()).await;
    let mut rx = f.core.subscribe();
    let dir = worker(&f, &mut rx).await;
    f.wt.put(ROOT, "b.rs", "mine\n");

    let (item, files, conflicts, written) = update(&f, false).await;
    assert_eq!(
        (files, conflicts.clone(), written),
        (0, vec!["b.rs".to_owned()], false)
    );
    assert_eq!(
        f.wt.files(&dir).get("b.rs").map(String::as_str),
        Some("worker b\n")
    );
    assert!(item.behind.is_some(), "still behind");

    let (item, files, conflicts, written) = update(&f, true).await;
    assert_eq!(
        (files, conflicts, written),
        (1, vec!["b.rs".to_owned()], true)
    );
    assert!(item.behind.is_none());
    let b = f.wt.files(&dir).get("b.rs").cloned().unwrap_or_default();
    assert!(b.starts_with("<<<<<<<"), "{b}");

    ok(
        &f.core,
        json!({"method": "review.comment", "params": {"id": "r1", "text": "fix it"}}),
    )
    .await;
    until(&mut rx, "s2", AgentStatus::Idle).await;
    let sent: Vec<(MessageFrom, String)> = transcript(&f.core, "s2")
        .await
        .into_iter()
        .filter_map(|m| match m {
            AgentMessage::UserText { text, from } => Some((from, text)),
            _ => None,
        })
        .collect();
    let note = &sent[sent.len() - 1];
    assert_eq!(note.0, MessageFrom::Ovra);
    assert!(
        note.1
            .contains("latest changes into your worktree (1 file)"),
        "{}",
        note.1
    );
    assert!(note.1.contains("conflict markers"), "{}", note.1);
    assert!(
        note.1.contains("<untrusted origin=\"file names\">b.rs<"),
        "{}",
        note.1
    );
}

#[tokio::test]
async fn an_update_waits_until_the_worker_is_idle() {
    let bash = Step::Use {
        tool: "Bash",
        input: json!({"command": "make"}),
    };
    let f = fx(vec![vec![Step::Say("on it")], vec![bash]]).await;
    let mut rx = f.core.subscribe();
    worker(&f, &mut rx).await;
    f.wt.put(ROOT, "c.rs", "new\n");
    ok(
        &f.core,
        json!({"method": "review.comment", "params": {"id": "r1", "text": "more"}}),
    )
    .await;
    until(&mut rx, "s2", AgentStatus::WaitingForApproval).await;
    let busy = call(
        &f.core,
        json!({"method": "review.update", "params": {"id": "r1"}}),
    )
    .await;
    assert_eq!(code(busy), ErrorCode::Conflict);
}

#[tokio::test]
async fn a_review_read_before_the_update_cannot_be_kept() {
    let f = fx(say()).await;
    let mut rx = f.core.subscribe();
    worker(&f, &mut rx).await;
    let before = approval(&f.core, "r1", json!(null)).await;
    f.wt.put(ROOT, "c.rs", "new\n");
    update(&f, false).await;
    assert_eq!(code(call(&f.core, before).await), ErrorCode::Conflict);
}

#[tokio::test]
async fn pushed_work_is_not_updated_under_its_pull_request() {
    let f = fx(say()).await;
    let mut rx = f.core.subscribe();
    worker(&f, &mut rx).await;
    f.wt.put(ROOT, "c.rs", "new\n");
    // A comment reopened a published item: it is open, but its pull request stays.
    f.core.orchestration_state().lock().reviews[0].pull_request =
        Some(crate::protocol::pr::PullRequest {
            number: 1,
            url: "https://github.com/o/r/pull/1".into(),
            remote: "origin".into(),
            repo: None,
            base: "main".into(),
            branch: "ovra/w".into(),
            pushed: "c1".into(),
            status: None,
        });
    let refused = call(
        &f.core,
        json!({"method": "review.update", "params": {"id": "r1"}}),
    )
    .await;
    assert_eq!(code(refused), ErrorCode::Conflict);
}

#[tokio::test]
async fn files_one_side_deleted_are_not_merged() {
    let f = fx(say()).await;
    let mut rx = f.core.subscribe();
    let dir = worker(&f, &mut rx).await;
    f.wt.delete(ROOT, "b.rs");
    let refused = call(
        &f.core,
        json!({"method": "review.update", "params": {"id": "r1", "conflicts": true}}),
    )
    .await;
    assert_eq!(code(refused), ErrorCode::Conflict);
    assert_eq!(
        f.wt.files(&dir).get("b.rs").map(String::as_str),
        Some("worker b\n")
    );
}

#[tokio::test]
async fn undecided_changes_of_the_direct_agent_stay_out_of_workers() {
    let f = DirectFx::start().await;
    f.wt.put(&f.worktree, "b.rs", "worker\n");
    f.direct_writes("d.rs", "direct\n").await;
    let item = f.worker_item().await;
    assert_eq!(item.behind.map(|b| b.files), Some(1));
    let refused = call(
        &f.core,
        json!({"method": "review.update", "params": {"id": item.id.0}}),
    )
    .await;
    assert_eq!(code(refused), ErrorCode::Conflict);
    assert!(!f.wt.files(&f.worktree).contains_key("d.rs"));
    // And a direct item is never behind.
    let direct = reviews(&f.core)
        .await
        .into_iter()
        .find(|i| i.mode == SessionMode::Direct);
    assert_eq!(direct.map(|d| d.behind), Some(None));
}
