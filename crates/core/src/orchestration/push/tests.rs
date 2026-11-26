use serde_json::json;
use tokio::sync::broadcast;

use super::super::publish::tests::{Fx, call, dir, files_of, fx_in, lock, ok, opened};
use crate::fakes::Step;
use crate::protocol::agents::{SandboxNetwork, SandboxReport, SessionId};
use crate::protocol::audit::AuditEvent;
use crate::protocol::orchestration::PushPlan;
use crate::protocol::permissions::{PermissionRequest, PermissionsEvent};
use crate::protocol::projects::ProjectId;
use crate::protocol::{ErrorCode, Event, ProtocolError};
use crate::services::{AuditLog, ToolCaller, ToolRole};
use crate::{PushArgs, Pushed};

fn caller(role: ToolRole) -> ToolCaller {
    let session = if role == ToolRole::Worker { "s2" } else { "s1" };
    ToolCaller {
        session: SessionId(session.into()),
        project: ProjectId("p1".into()),
        role,
    }
}

fn args(branch: Option<&str>) -> PushArgs {
    PushArgs {
        message: "Add b\n\nThe second file.".into(),
        branch: branch.map(str::to_owned),
        remote: None,
        base: None,
    }
}

/// A worker `w` (session `s2`) that added `b.rs`, sandboxed without a
/// network, so `auto` rules count.
async fn fx() -> Fx {
    fx_on(SandboxNetwork::Blocked).await
}

async fn fx_on(network: SandboxNetwork) -> Fx {
    let sandbox = SandboxReport {
        name: "test".into(),
        network,
        gaps: Vec::new(),
    };
    fx_in(vec![vec![Step::Say("done")]], Some(sandbox)).await
}

fn allow_ovra_branches() -> serde_json::Value {
    json!({"method": "policy.set", "params": {"project": "p1",
        "policy": {"rules": [{"tool": "Push", "match": "origin ovra/*", "action": "auto"}]}}})
}

/// The next push the person is asked about.
async fn asked(rx: &mut broadcast::Receiver<Event>) -> PermissionRequest {
    loop {
        match crate::Core::next_event(rx).await {
            Some(Event::Permissions(PermissionsEvent::Requested(request))) => return *request,
            Some(_) => {}
            None => panic!("core closed"),
        }
    }
}

/// Starts the worker's push, answers its card with `answer` (allow,
/// always or deny) and returns what the card showed and the push's result.
async fn answered(
    f: &Fx,
    branch: Option<&str>,
    answer: &str,
) -> (PermissionRequest, Result<Pushed, ProtocolError>) {
    let mut rx = f.core.subscribe();
    let core = f.core.clone();
    let branch = branch.map(str::to_owned);
    let push = tokio::spawn(async move {
        let args = args(branch.as_deref());
        core.push_work(&caller(ToolRole::Worker), args).await
    });
    let request = asked(&mut rx).await;
    let params = match answer {
        "always" => json!({"id": request.id, "decision": "allow", "always": true}),
        "deny" => json!({"id": request.id, "decision": "deny", "message": "not yet"}),
        _ => json!({"id": request.id, "decision": "allow"}),
    };
    let sent = call(
        &f.core,
        json!({"method": "permissions.answer", "params": params}),
    )
    .await;
    assert!(sent.is_ok(), "{sent:?}");
    let result = push.await.unwrap_or_else(|e| panic!("{e}"));
    (request, result)
}

fn plan(request: &PermissionRequest) -> PushPlan {
    serde_json::from_value(request.input.clone()).unwrap_or_else(|e| panic!("{e}"))
}

#[tokio::test]
async fn a_worker_pushes_one_commit_of_its_changes_once_the_person_allows() {
    let f = fx().await;
    let main = lock(&f.forge.branches)
        .get(&("origin".to_owned(), "main".to_owned()))
        .cloned()
        .unwrap_or_default();
    let (request, pushed) = answered(&f, None, "allow").await;
    assert_eq!(request.tool, "Push");
    assert_eq!(request.title.as_deref(), Some("Push to origin/ovra/t1/w"));
    let shown = plan(&request);
    assert_eq!(
        (
            shown.remote.as_str(),
            shown.host.as_deref(),
            shown.branch.as_str()
        ),
        ("origin", Some("github.com"), "ovra/t1/w")
    );
    assert!(shown.new_branch && !shown.protected_branch);
    assert_eq!(
        (shown.base.as_deref(), shown.onto.as_str()),
        (Some("main"), main.as_str())
    );
    let files: Vec<_> = shown.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(files, ["b.rs"]);
    assert_eq!(shown.message, "Add b\n\nThe second file.");
    assert_eq!(
        request.always.as_ref().map(crate::policy::text).as_deref(),
        Some("Push(origin ovra/*)")
    );

    let pushed = pushed.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(pushed.commit, shown.commit);
    assert_eq!(pushed.base.as_deref(), Some("main"));
    assert_eq!(
        *lock(&f.forge.pushed),
        [(
            "origin".to_owned(),
            shown.commit.clone(),
            "ovra/t1/w".to_owned()
        )]
    );
    // What went out is the main checkout plus the worker's file.
    let mut files = files_of(&f.wt, &shown.commit).await;
    files.sort();
    let files: Vec<_> = files.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(files, ["a.rs", "b.rs"]);
    let range = crate::protocol::audit::AuditRange::default();
    let events = f.audit.list(&ProjectId("p1".into()), range, 100);
    let events = events.unwrap_or_default();
    assert!(events.iter().any(|e| matches!(
        &e.event,
        AuditEvent::BranchPushed { branch, .. } if branch == "ovra/t1/w"
    )));
    assert!(events.iter().any(|e| matches!(
        &e.event,
        AuditEvent::ToolDecided { tool, allowed: true, .. } if tool == "Push"
    )));
}

#[tokio::test]
async fn always_allow_adds_a_rule_and_the_next_push_builds_on_the_last() {
    let f = fx().await;
    let (_, first) = answered(&f, None, "always").await;
    let first = first.unwrap_or_else(|e| panic!("{e}"));
    let policy = f.core.project_store().policy(&ProjectId("p1".into()));
    let rules: Vec<_> = policy
        .unwrap_or_default()
        .rules
        .iter()
        .map(crate::policy::text)
        .collect();
    assert_eq!(rules, ["Push(origin ovra/*)"]);

    f.wt.put(dir(), "c.rs", "three\n");
    let again = f
        .core
        .push_work(&caller(ToolRole::Worker), args(None))
        .await;
    let again = again.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(again.base, None);
    let pushed = lock(&f.forge.pushed).clone();
    assert_eq!(pushed.len(), 2);
    let mut files = files_of(&f.wt, &again.commit).await;
    files.sort();
    let files: Vec<_> = files.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(files, ["a.rs", "b.rs", "c.rs"]);
    assert_ne!(again.commit, first.commit);

    // Nothing new: refused, nothing pushed.
    let same = f
        .core
        .push_work(&caller(ToolRole::Worker), args(None))
        .await;
    assert_eq!(same.err().map(|e| e.code), Some(ErrorCode::Conflict));
    assert_eq!(lock(&f.forge.pushed).len(), 2);
}

#[tokio::test]
async fn a_shared_branch_always_asks_and_offers_no_rule() {
    let f = fx().await;
    let wide = json!({"method": "policy.set", "params": {"project": "p1",
        "policy": {"rules": [{"tool": "Push", "action": "auto"}]}}});
    ok(&f.core, wide).await;
    let (request, pushed) = answered(&f, Some("main"), "allow").await;
    let shown = plan(&request);
    assert!(shown.protected_branch && !shown.new_branch);
    assert_eq!(request.always, None);
    assert!(pushed.is_ok(), "{pushed:?}");
    // A fast-forward of main.
    assert_eq!(lock(&f.forge.pushed)[0].2, "main");
}

#[tokio::test]
async fn a_denied_push_sends_nothing_and_says_why() {
    let f = fx().await;
    let (_, pushed) = answered(&f, None, "deny").await;
    let refused = pushed.err().unwrap_or_else(|| panic!("pushed"));
    assert_eq!(refused.code, ErrorCode::Forbidden);
    assert!(refused.message.contains("not yet"), "{}", refused.message);
    assert!(lock(&f.forge.pushed).is_empty());

    let deny = json!({"method": "policy.set", "params": {"project": "p1",
        "policy": {"rules": [{"tool": "Push", "match": "origin ovra/*", "action": "deny"}]}}});
    ok(&f.core, deny).await;
    let pushed = f
        .core
        .push_work(&caller(ToolRole::Worker), args(None))
        .await;
    assert_eq!(pushed.err().map(|e| e.code), Some(ErrorCode::Forbidden));
    assert!(lock(&f.forge.pushed).is_empty());
}

#[tokio::test]
async fn protected_files_coordinators_and_bad_names_are_refused_before_asking() {
    let f = fx().await;
    let worker = caller(ToolRole::Worker);
    let boss = caller(ToolRole::Coordinator);
    let coordinator = f.core.push_work(&boss, args(None)).await;
    assert_eq!(
        coordinator.err().map(|e| e.code),
        Some(ErrorCode::Forbidden)
    );
    for branch in ["--force", "a..b", "x:refs/heads/main"] {
        let bad = f.core.push_work(&worker, args(Some(branch))).await;
        assert_eq!(
            bad.err().map(|e| e.code),
            Some(ErrorCode::BadRequest),
            "{branch}"
        );
    }
    let url = PushArgs {
        remote: Some("https://x:ghp_SECRET@github.com/o/r".into()),
        ..args(None)
    };
    let bad = f.core.push_work(&worker, url).await.err();
    assert!(bad.is_some_and(|e| !e.message.contains("SECRET")));
    for message in ["", &"x".repeat(73)] {
        let long = PushArgs {
            message: message.to_owned(),
            ..args(None)
        };
        let bad = f.core.push_work(&worker, long).await;
        assert_eq!(bad.err().map(|e| e.code), Some(ErrorCode::BadRequest));
    }

    f.wt.put(dir(), ".github/workflows/ci.yml", "on: push\n");
    let pushed = f.core.push_work(&worker, args(None)).await;
    let refused = pushed.err().unwrap_or_else(|| panic!("pushed"));
    assert_eq!(refused.code, ErrorCode::Forbidden);
    assert!(refused.message.contains(".github/workflows/ci.yml"));
    assert!(lock(&f.forge.pushed).is_empty());
}

#[tokio::test]
async fn a_rule_pushes_only_to_the_workers_own_branch_and_only_where_contained() {
    let f = fx().await;
    ok(&f.core, allow_ovra_branches()).await;
    // Another branch under ovra/ asks, and offers no rule.
    let (request, pushed) = answered(&f, Some("ovra/t1/other"), "allow").await;
    assert!(!plan(&request).own_branch);
    assert_eq!(request.always, None);
    assert!(pushed.is_ok(), "{pushed:?}");

    let open = fx_on(SandboxNetwork::Open).await;
    ok(&open.core, allow_ovra_branches()).await;
    let (request, pushed) = answered(&open, None, "allow").await;
    assert!(plan(&request).own_branch);
    assert_eq!(request.always, None);
    assert!(pushed.is_ok(), "{pushed:?}");
}

#[tokio::test]
async fn another_workers_branch_is_not_the_workers_to_take() {
    let f = fx().await;
    let boss = caller(ToolRole::Coordinator);
    let other = f.core.spawn_agent(&boss, "v", "add v", false).await;
    let other = other.unwrap_or_else(|e| panic!("{e}"));
    let worker = caller(ToolRole::Worker);
    let taken = f.core.push_work(&worker, args(Some(&other.branch))).await;
    let taken = taken.err().unwrap_or_else(|| panic!("pushed"));
    assert_eq!(taken.code, ErrorCode::Forbidden);
    assert!(taken.message.contains("worker v"), "{}", taken.message);
    assert!(lock(&f.forge.pushed).is_empty());
}

#[tokio::test]
async fn a_push_to_the_items_pull_request_moves_its_lease_but_not_what_was_reviewed() {
    let f = fx().await;
    let (id, pr) = opened(&f).await;
    let reviewed = |core: &crate::Core| {
        let state = core.orchestration_state().lock();
        let entry = state.reviews.iter().find(|r| r.id.0 == id);
        entry.map(|e| {
            (
                e.published_from.clone(),
                e.pull_request.clone().map(|p| p.pushed),
            )
        })
    };
    let (before, _) = reviewed(&f.core).unwrap_or_default();
    f.wt.put(dir(), "c.rs", "three\n");
    ok(&f.core, allow_ovra_branches()).await;
    let pushed = f
        .core
        .push_work(&caller(ToolRole::Worker), args(Some(&pr.branch)))
        .await;
    let pushed = pushed.unwrap_or_else(|e| panic!("{e}"));
    let (after, lease) = reviewed(&f.core).unwrap_or_default();
    assert_eq!(after, before);
    assert_eq!(lease, Some(pushed.commit));
}

#[tokio::test]
async fn a_branch_that_moved_while_the_person_looked_is_not_overwritten() {
    let f = fx().await;
    let mut rx = f.core.subscribe();
    let core = f.core.clone();
    let push = tokio::spawn(async move {
        core.push_work(&caller(ToolRole::Worker), args(Some("main")))
            .await
    });
    let request = asked(&mut rx).await;
    lock(&f.forge.branches).insert(("origin".into(), "main".into()), "someone-else".into());
    let params = json!({"id": request.id, "decision": "allow"});
    let sent = call(
        &f.core,
        json!({"method": "permissions.answer", "params": params}),
    )
    .await;
    assert!(sent.is_ok());
    let refused = push.await.unwrap_or_else(|e| panic!("{e}")).err();
    let refused = refused.unwrap_or_else(|| panic!("pushed"));
    assert_eq!(refused.code, ErrorCode::Conflict);
    assert!(
        refused.message.contains("Call push again"),
        "{}",
        refused.message
    );
}

#[tokio::test]
async fn review_trailers_hidden_text_and_remotes_without_a_host_are_refused() {
    let f = fx().await;
    let worker = caller(ToolRole::Worker);
    for message in [
        "Add b\n\nReviewed-by: someone <a@b>",
        "Add b\n\nSigned-off-by: someone <a@b>",
        "Add \u{202e}b",
        "Add \u{1b}[31mb",
    ] {
        let bad = PushArgs {
            message: message.to_owned(),
            ..args(None)
        };
        let bad = f.core.push_work(&worker, bad).await;
        assert_eq!(
            bad.err().map(|e| e.code),
            Some(ErrorCode::BadRequest),
            "{message:?}"
        );
    }
    lock(&f.forge.gh).host = None;
    let local = f.core.push_work(&worker, args(None)).await;
    assert_eq!(local.err().map(|e| e.code), Some(ErrorCode::Forbidden));
    assert!(lock(&f.forge.pushed).is_empty());
}

#[tokio::test]
async fn a_file_changed_again_after_a_push_goes_out_in_the_next_push_and_update() {
    let f = fx().await;
    let (_, pr) = opened(&f).await;
    ok(&f.core, allow_ovra_branches()).await;
    let worker = caller(ToolRole::Worker);
    for text in ["two v2\n", "two v3\n"] {
        f.wt.put(dir(), "b.rs", text);
        let pushed = f.core.push_work(&worker, args(Some(&pr.branch))).await;
        let pushed = pushed.unwrap_or_else(|e| panic!("{e}"));
        let files = files_of(&f.wt, &pushed.commit).await;
        assert!(
            files.contains(&("b.rs".to_owned(), text.to_owned())),
            "{files:?}"
        );
    }
    let id = {
        let state = f.core.orchestration_state().lock();
        state
            .reviews
            .iter()
            .find(|r| r.session.0 == "s2")
            .map(|r| r.id.0.clone())
    };
    let id = id.unwrap_or_default();
    f.wt.put(dir(), "b.rs", "two v4\n");
    let item = super::super::publish::tests::item(&f.core).await;
    let update =
        json!({"method": "pr.update", "params": {"id": id, "fingerprint": item.fingerprint}});
    let updated = call(&f.core, update).await;
    assert!(updated.is_ok(), "{updated:?}");
}
