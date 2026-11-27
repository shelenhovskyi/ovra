//! `review.undo_keep`: a keep without a commit can be taken back.

use super::*;

async fn undo_keep(core: &Core, id: &str) -> (ReviewItem, Vec<String>, Vec<String>) {
    let done = ok(
        core,
        json!({"method": "review.undo_keep", "params": {"id": id}}),
    )
    .await;
    let Response::Review(ReviewResponse::Done {
        item,
        conflicts,
        restored,
    }) = done
    else {
        panic!("unexpected {done:?}");
    };
    (*item, conflicts, restored)
}

fn item_of(items: &[ReviewItem], id: &str) -> ReviewItem {
    items
        .iter()
        .find(|i| i.id.0 == id)
        .cloned()
        .unwrap_or_else(|| panic!("no item {id}"))
}

/// A task with one idle worker that changed `files` in its worktree.
async fn worker_with(f: &Fx, name: &str, session: &str, files: &[(&str, &str)]) -> String {
    let mut rx = f.core.subscribe();
    let boss = caller("s1", ToolRole::Coordinator);
    let worker = f.core.spawn_agent(&boss, name, "x", false).await;
    let worker = worker.unwrap_or_else(|e| panic!("{e}"));
    until(&mut rx, session, AgentStatus::Idle).await;
    for (path, text) in files {
        f.wt.put(&worker.worktree, path, text);
    }
    worker.worktree
}

#[tokio::test]
async fn a_kept_workers_copy_can_be_taken_back_and_the_review_opens_again() {
    let f = fx(say()).await;
    start_task(&f.core, 3).await;
    f.wt.put(ROOT, "a.rs", "mine\n");
    worker_with(&f, "w", "s2", &[("a.rs", "worker\n"), ("b.rs", "new\n")]).await;
    ok(&f.core, approval(&f.core, "r1", json!(null)).await).await;
    let kept = item_of(&reviews(&f.core).await, "r1");
    assert_eq!(kept.state, ReviewState::Approved);
    assert!(kept.can_undo_keep);
    assert_eq!(
        f.wt.files(ROOT).get("b.rs").map(String::as_str),
        Some("new\n")
    );

    let (item, conflicts, mut restored) = undo_keep(&f.core, "r1").await;
    restored.sort();
    assert_eq!(item.state, ReviewState::Open);
    assert!(!item.can_undo_keep);
    assert!(conflicts.is_empty());
    assert_eq!(restored, ["a.rs", "b.rs"]);
    assert_eq!(
        f.wt.files(ROOT).get("a.rs").map(String::as_str),
        Some("mine\n")
    );
    assert_eq!(f.wt.files(ROOT).get("b.rs"), None);
    let mut paths: Vec<String> = item.files.iter().map(|c| c.path.clone()).collect();
    paths.sort();
    assert_eq!(paths, ["a.rs", "b.rs"], "the same changes wait again");
    assert_eq!(
        audited(&f).last().map(|(_, e)| e.clone()),
        Some(AuditEvent::ReviewDecided {
            review: ReviewId("r1".into()),
            action: ReviewAction::UndoneKeep,
        })
    );
    let twice = call(
        &f.core,
        json!({"method": "review.undo_keep", "params": {"id": "r1"}}),
    )
    .await;
    assert_eq!(code(twice), ErrorCode::Conflict);
    // And it can be kept again.
    ok(&f.core, approval(&f.core, "r1", json!(null)).await).await;
    assert_eq!(
        f.wt.files(ROOT).get("b.rs").map(String::as_str),
        Some("new\n")
    );
}

#[tokio::test]
async fn undoing_a_keep_leaves_what_the_person_changed_since() {
    let f = fx(say()).await;
    start_task(&f.core, 3).await;
    worker_with(&f, "w", "s2", &[("b.rs", "new\n"), ("c.rs", "also\n")]).await;
    ok(&f.core, approval(&f.core, "r1", json!(null)).await).await;
    f.wt.put(ROOT, "c.rs", "edited by me\n");
    let (_, conflicts, restored) = undo_keep(&f.core, "r1").await;
    assert_eq!(
        (conflicts, restored),
        (vec!["c.rs".to_owned()], vec!["b.rs".to_owned()])
    );
    assert_eq!(
        f.wt.files(ROOT).get("c.rs").map(String::as_str),
        Some("edited by me\n")
    );
}

#[tokio::test]
async fn a_keep_that_committed_or_was_followed_by_another_cannot_be_undone() {
    let f = fx(say()).await;
    start_task(&f.core, 3).await;
    worker_with(&f, "one", "s2", &[("b.rs", "one\n")]).await;
    worker_with(&f, "two", "s3", &[("c.rs", "two\n")]).await;
    ok(&f.core, approval(&f.core, "r1", json!(null)).await).await;
    assert!(item_of(&reviews(&f.core).await, "r1").can_undo_keep);
    ok(&f.core, approval(&f.core, "r2", json!("Add c")).await).await;
    let items = reviews(&f.core).await;
    assert!(
        !item_of(&items, "r1").can_undo_keep,
        "the later keep ends it"
    );
    assert!(
        !item_of(&items, "r2").can_undo_keep,
        "a commit is not undone"
    );
    for id in ["r1", "r2"] {
        let undo = call(
            &f.core,
            json!({"method": "review.undo_keep", "params": {"id": id}}),
        )
        .await;
        assert_eq!(code(undo), ErrorCode::Conflict);
    }
}

#[tokio::test]
async fn the_workers_new_work_ends_the_undo_of_its_keep() {
    let f = fx(say()).await;
    start_task(&f.core, 3).await;
    let tree = worker_with(&f, "w", "s2", &[("b.rs", "one\n")]).await;
    ok(&f.core, approval(&f.core, "r1", json!(null)).await).await;
    f.wt.put(&tree, "b.rs", "two\n");
    let undo = call(
        &f.core,
        json!({"method": "review.undo_keep", "params": {"id": "r1"}}),
    )
    .await;
    assert_eq!(code(undo), ErrorCode::Conflict);
    // Listing opens the new work as its own item, which ends the undo.
    let items = reviews(&f.core).await;
    assert!(!item_of(&items, "r1").can_undo_keep);
    assert_eq!(item_of(&items, "r2").state, ReviewState::Open);
}

#[tokio::test]
async fn once_the_workers_folder_is_gone_an_undone_keep_counts_as_discarded() {
    let f = fx(say()).await;
    let mut rx = f.core.subscribe();
    start_task(&f.core, 3).await;
    let tree = worker_with(&f, "w", "s2", &[("b.rs", "new\n")]).await;
    ok(&f.core, approval(&f.core, "r1", json!(null)).await).await;
    ok(
        &f.core,
        json!({"method": "orchestration.stop", "params": {"task": "t1"}}),
    )
    .await;
    until(&mut rx, "s1", AgentStatus::Stopped).await;
    ok(
        &f.core,
        json!({"method": "orchestration.list", "params": {"project": "p1"}}),
    )
    .await;
    assert!(!f.wt.has(&tree), "a keep does not hold the worktree");
    let (item, _, restored) = undo_keep(&f.core, "r1").await;
    assert_eq!(item.state, ReviewState::Rejected);
    assert_eq!(restored, ["b.rs"]);
    assert_eq!(f.wt.files(ROOT).get("b.rs"), None);
}

#[tokio::test]
async fn a_kept_direct_review_opens_again_and_can_then_be_discarded() {
    let f = DirectFx::start().await;
    f.direct_writes("a.rs", "agent\n").await;
    let item = idle_direct_item(&f).await;
    let id = item.id.0.clone();
    ok(
        &f.core,
        json!({"method": "review.approve", "params": {"id": id, "commit": null}}),
    )
    .await;
    let kept = item_of(&reviews(&f.core).await, &id);
    assert_eq!(kept.state, ReviewState::Approved);
    assert!(kept.can_undo_keep);

    let (item, conflicts, restored) = undo_keep(&f.core, &id).await;
    assert!(
        conflicts.is_empty() && restored.is_empty(),
        "nothing on disk changes"
    );
    assert_eq!(item.state, ReviewState::Open);
    assert_eq!(
        item.files
            .iter()
            .map(|c| c.path.as_str())
            .collect::<Vec<_>>(),
        ["a.rs"]
    );
    // The agent's attribution came back: a discard reverts its file.
    ok(
        &f.core,
        json!({"method": "review.reject", "params": {"id": id}}),
    )
    .await;
    assert_eq!(
        f.wt.files(ROOT).get("a.rs").map(String::as_str),
        Some("one\n")
    );
}

#[tokio::test]
async fn a_direct_keep_is_not_undone_once_the_agent_changed_files_again() {
    let f = DirectFx::start().await;
    f.direct_writes("a.rs", "agent\n").await;
    let id = idle_direct_item(&f).await.id.0;
    ok(
        &f.core,
        json!({"method": "review.approve", "params": {"id": id, "commit": "Keep it"}}),
    )
    .await;
    assert!(
        !item_of(&reviews(&f.core).await, &id).can_undo_keep,
        "committed"
    );
    f.direct_writes("a.rs", "more\n").await;
    let next = idle_direct_item(&f).await.id.0;
    ok(
        &f.core,
        json!({"method": "review.approve", "params": {"id": next, "commit": null}}),
    )
    .await;
    f.direct_writes("a.rs", "again\n").await;
    let undo = call(
        &f.core,
        json!({"method": "review.undo_keep", "params": {"id": next}}),
    )
    .await;
    assert_eq!(code(undo), ErrorCode::Conflict);
}

#[tokio::test]
async fn a_later_discard_in_the_project_ends_the_undo_of_a_keep() {
    let f = fx(say()).await;
    start_task(&f.core, 3).await;
    worker_with(&f, "one", "s2", &[("b.rs", "one\n")]).await;
    worker_with(&f, "two", "s3", &[("c.rs", "two\n")]).await;
    ok(&f.core, approval(&f.core, "r1", json!(null)).await).await;
    ok(
        &f.core,
        json!({"method": "review.reject", "params": {"id": "r2"}}),
    )
    .await;
    assert!(!item_of(&reviews(&f.core).await, "r1").can_undo_keep);
}
