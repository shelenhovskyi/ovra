//! One task across several repositories (`docs/specs/multi-repo.md`).

use super::*;

const API: &str = crate::test_path!("/work/api");
const PLAIN: &str = crate::test_path!("/work/plain");

/// [`fx`] with project `p2` at [`API`], a repository named `api`.
async fn fx_api() -> Fx {
    let f = fx(say()).await;
    f.wt.put(API, "lib.rs", "pub fn v1() {}\n");
    ok(
        &f.core,
        json!({"method": "projects.add", "params": {"root": API}}),
    )
    .await;
    f
}

fn linked(repos: serde_json::Value) -> serde_json::Value {
    json!({"method": "orchestration.start", "params": {
        "project": "p1", "prompt": "rename v1 everywhere", "solo": true, "repos": repos}})
}

async fn list(core: &Core, project: &str) -> Vec<ReviewItem> {
    match ok(
        core,
        json!({"method": "review.list", "params": {"project": project}}),
    )
    .await
    {
        Response::Review(ReviewResponse::List(items)) => items,
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn one_agent_changes_two_repositories_each_in_its_own_copy() {
    let f = fx_api().await;
    let mut rx = f.core.subscribe();
    let Response::Orchestration(OrchestrationResponse::Task(task)) =
        ok(&f.core, linked(json!(["p2"]))).await
    else {
        panic!("no task");
    };
    let worker = &task.workers[0];
    let [copy] = worker.repos.as_slice() else {
        panic!("one linked copy: {worker:?}");
    };
    assert_eq!(copy.project, ProjectId("p2".into()));
    assert_eq!(copy.branch, format!("ovra/{}/agent", task.id.0));
    let copy_dir = PathBuf::from(WORKTREES)
        .join("p2")
        .join(&task.id.0)
        .join("agent");
    assert_eq!(PathBuf::from(&copy.worktree), copy_dir);
    assert!(f.wt.has(&copy.worktree));
    until(&mut rx, &worker.session.0, AgentStatus::Idle).await;

    // The agent adds the copy, its sandbox writes it and reads its git folder.
    let spec = f
        .agent
        .started
        .lock()
        .map(|s| s[0].clone())
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(spec.add_dirs, std::slice::from_ref(&copy_dir));
    assert_eq!(spec.sandbox.more_work_dirs, std::slice::from_ref(&copy_dir));
    assert!(spec.sandbox.read.contains(&PathBuf::from(API).join(".git")));
    // Its first message says where the copy is, as Ovra's.
    let first = transcript(&f.core, &worker.session.0).await;
    assert!(
        first.iter().any(|m| matches!(m, AgentMessage::UserText { text, from: MessageFrom::Ovra }
            if text.contains("other repositories") && text.contains(&format!("- api: {}", copy.worktree)))),
        "{first:?}"
    );

    // One item per repository, each in its own project, listed in both.
    f.wt.put(&worker.worktree, "a.rs", "two\n");
    f.wt.put(&copy.worktree, "lib.rs", "pub fn v2() {}\n");
    let items = list(&f.core, "p1").await;
    assert_eq!(items.len(), 2, "{items:?}");
    let repos = vec![ProjectId("p1".into()), ProjectId("p2".into())];
    assert!(
        items
            .iter()
            .all(|i| *i.task_repos == *repos && i.session == worker.session)
    );
    let api_item = items.iter().find(|i| i.project.0 == "p2").cloned();
    let api_item = api_item.unwrap_or_else(|| panic!("no api item: {items:?}"));
    assert_eq!(api_item.files.len(), 1);
    assert_eq!(api_item.files[0].path, "lib.rs");
    assert_eq!(list(&f.core, "p2").await.len(), 2);

    // While it works, its copy is no leftover of api, and api stays listed.
    let leftovers = json!({"method": "orchestration.leftovers", "params": {"project": "p2"}});
    let Response::Orchestration(OrchestrationResponse::Leftovers(left)) =
        ok(&f.core, leftovers).await
    else {
        panic!("no leftovers");
    };
    assert!(left.is_empty(), "{left:?}");
    let remove = json!({"method": "projects.remove", "params": {"id": "p2"}});
    assert_eq!(code(call(&f.core, remove).await), ErrorCode::Conflict);

    // The main item first: nothing lands in api.
    let main_item = items.iter().find(|i| i.project.0 == "p1").cloned();
    let main_item = main_item.unwrap_or_else(|| panic!("no main item"));
    let reject = json!({"method": "review.reject", "params": {"id": main_item.id.0}});
    ok(&f.core, reject).await;
    assert_eq!(
        f.wt.files(API).get("lib.rs").map(String::as_str),
        Some("pub fn v1() {}\n")
    );
    // Stopped with one repository still undecided, both copies stay.
    ok(
        &f.core,
        json!({"method": "orchestration.stop", "params": {"task": task.id.0}}),
    )
    .await;
    until(&mut rx, &worker.session.0, AgentStatus::Stopped).await;
    let tasks = json!({"method": "orchestration.list", "params": {"project": "p1"}});
    ok(&f.core, tasks).await;
    assert!(f.wt.has(&copy.worktree) && f.wt.has(&worker.worktree));
    // Keeping the api item last lands in api only, and both copies go with
    // their branches, without another look at the task.
    let keep = json!({"method": "review.approve", "params": {
        "id": api_item.id.0, "commit": null, "fingerprint": api_item.fingerprint}});
    ok(&f.core, keep).await;
    assert_eq!(
        f.wt.files(API).get("lib.rs").map(String::as_str),
        Some("pub fn v2() {}\n")
    );
    assert_eq!(
        f.wt.files(ROOT).get("a.rs").map(String::as_str),
        Some("one\n")
    );
    assert!(!f.wt.has(&copy.worktree) && !f.wt.has(&worker.worktree));
    let deleted =
        f.wt.deleted_branches
            .lock()
            .map(|d| d.clone())
            .unwrap_or_default();
    assert_eq!(deleted.len(), 2, "{deleted:?}");
    // Now api can go.
    let remove = json!({"method": "projects.remove", "params": {"id": "p2"}});
    ok(&f.core, remove).await;
}

#[tokio::test]
async fn linked_repositories_are_refused_unless_they_fit() {
    let f = fx_api().await;
    ok(
        &f.core,
        json!({"method": "projects.add", "params": {"root": PLAIN}}),
    )
    .await;
    let refused = |params: serde_json::Value| {
        let core = f.core.clone();
        async move {
            let mut start = json!({"project": "p1", "prompt": "x", "solo": true});
            if let (Some(start), Some(params)) = (start.as_object_mut(), params.as_object()) {
                start.extend(params.clone());
            }
            call(
                &core,
                json!({"method": "orchestration.start", "params": start}),
            )
            .await
            .err()
            .map(|e| (e.code, e.message))
        }
    };
    let bad = ErrorCode::BadRequest;
    let cases = [
        (
            json!({"repos": ["p2"], "solo": false}),
            "only one agent in its own copy",
        ),
        (json!({"repos": ["p1"]}), "once"),
        (json!({"repos": ["p2", "p2"]}), "once"),
        (json!({"repos": ["p3"]}), "plain is not a git repository"),
        (json!({"repos": ["p2"], "agent": "codex"}), "only Claude"),
        (json!({"repos": ["p2", "p4", "p5", "p6"]}), "at most 3"),
    ];
    for (params, says) in cases {
        let (code, message) = refused(params.clone())
            .await
            .unwrap_or_else(|| panic!("{params}"));
        assert_eq!(code, bad, "{params}: {message}");
        assert!(message.contains(says), "{params}: {message}");
    }
    let unknown = refused(json!({"repos": ["p9"]})).await.map(|e| e.0);
    assert_eq!(unknown, Some(ErrorCode::NotFound));
    assert!(
        f.agent
            .started
            .lock()
            .map(|s| s.is_empty())
            .unwrap_or(false)
    );
    assert!(!f.wt.has(format!("{WORKTREES}/p1/t1/agent")));
}

#[tokio::test]
async fn a_linked_project_with_untrusted_agent_settings_is_refused() {
    let f = fx_api().await;
    let config = crate::protocol::agents::ProjectAgentConfig {
        fingerprint: "f1".into(),
        items: vec![crate::protocol::agents::ConfigItem {
            path: ".claude/commands/x.md".into(),
            kind: crate::protocol::agents::ConfigKind::Commands,
            entries: vec!["x".into()],
        }],
    };
    if let Ok(mut configs) = f.agent.configs.lock() {
        configs.insert(PathBuf::from(API), config);
    }
    let err = call(&f.core, linked(json!(["p2"]))).await.err();
    assert!(
        err.as_ref()
            .is_some_and(|e| e.code == ErrorCode::BadRequest && e.message.contains("not trusted")),
        "{err:?}"
    );
    // Trusted as it is, it may go.
    ok(
        &f.core,
        json!({"method": "projects.trust", "params": {"id": "p2", "trusted": true, "fingerprint": "f1"}}),
    )
    .await;
    ok(&f.core, linked(json!(["p2"]))).await;
}
