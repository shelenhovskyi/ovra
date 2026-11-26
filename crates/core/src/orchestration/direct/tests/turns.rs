//! Checkpoints and going back (`docs/specs/checkpoints.md`).

use super::*;

impl Harness {
    /// Message `turn` is sent; its before snapshot is taken.
    async fn turn(&self, turn: u32) {
        let message = format!("message {turn}");
        assert!(self.direct.turn_started(&self.session, turn, message));
        self.direct.catch_up(&self.session).await;
    }

    /// The running turn reaches its result.
    async fn turn_end(&self) {
        assert!(self.direct.turn_ended(&self.session));
        self.direct.catch_up(&self.session).await;
    }

    /// The agent runs a shell command that does `change`.
    async fn agent_runs(&self, id: &str, change: impl FnOnce(&Self)) {
        self.started(id, "Bash", json!({"command": "make"})).await;
        change(self);
        self.ended(id).await;
    }

    async fn go_back(&self, turn: u32) -> (Vec<String>, Vec<String>) {
        let went = self.direct.go_back(&self.session, turn).await;
        went.unwrap_or_else(|e| panic!("{e:?}"))
    }

    async fn turns(&self) -> Vec<u32> {
        let list = self.direct.checkpoints(&self.session).await;
        let list = list.unwrap_or_else(|e| panic!("{e:?}"));
        list.into_iter().map(|c| c.turn).collect()
    }
}

fn strings(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

#[tokio::test]
async fn going_back_undoes_what_a_shell_call_changed() {
    let h = Harness::start(&[("b.txt", "keep me")]).await;
    h.turn(1).await;
    h.agent_runs("t1", |h| {
        h.put("a.txt", "generated");
        h.delete("b.txt");
    })
    .await;
    h.turn_end().await;
    let list = h.direct.checkpoints(&h.session).await.unwrap_or_default();
    assert_eq!(list.len(), 1);
    assert_eq!(
        (
            list[0].turn,
            list[0].message.as_str(),
            list[0].files_changed
        ),
        (1, "message 1", 2)
    );

    assert_eq!(h.go_back(1).await, (strings(&["a.txt", "b.txt"]), vec![]));
    assert_eq!(h.text("a.txt"), None);
    assert_eq!(h.text("b.txt").as_deref(), Some("keep me"));
    let note = h.direct.note(&h.session).unwrap_or_default();
    assert_eq!(
        note,
        "The files were set back to how they were before your turn 1; changes from turn 1 are undone."
    );
    // Restored files are still the agent's: nothing is left to review.
    assert_eq!(h.by().await, by(&[]));
}

#[tokio::test]
async fn a_file_the_person_changed_after_the_checkpoint_is_kept_and_listed() {
    let h = Harness::start(&[("p.txt", "old"), ("q.txt", "old")]).await;
    // Before turn 1: the person's edit is part of the checkpoint.
    h.put("q.txt", "person before");
    h.turn(1).await;
    h.agent_runs("t1", |h| {
        h.put("p.txt", "agent");
        h.put("q.txt", "agent");
    })
    .await;
    h.turn_end().await;
    h.turn(2).await;
    h.agent_runs("t2", |h| h.put("r.txt", "agent")).await;
    h.turn_end().await;
    // While the agent is idle, after both checkpoints.
    h.put("p.txt", "person after");

    let (restored, kept) = h.go_back(1).await;
    assert_eq!(
        (restored, kept),
        (strings(&["q.txt", "r.txt"]), strings(&["p.txt"]))
    );
    assert_eq!(h.text("p.txt").as_deref(), Some("person after"));
    assert_eq!(h.text("q.txt").as_deref(), Some("person before"));
    assert_eq!(h.text("r.txt"), None);
    let note = h.direct.note(&h.session).unwrap_or_default();
    assert!(
        note.ends_with(
            "changes from turns 1 to 2 are undone. Files the person changed are kept:\n\
             <untrusted origin=\"file names\">p.txt</untrusted>"
        ),
        "{note}"
    );
}

#[tokio::test]
async fn undo_puts_the_agents_files_back_unless_they_changed_since() {
    let h = Harness::start(&[]).await;
    h.turn(1).await;
    h.agent_runs("t1", |h| {
        h.put("a.txt", "agent");
        h.put("b.txt", "agent");
    })
    .await;
    h.turn_end().await;
    h.go_back(1).await;
    h.put("b.txt", "person");

    let undone = h.direct.undo_go_back(&h.session).await;
    let undone = undone.unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(undone, (strings(&["a.txt"]), strings(&["b.txt"])));
    assert_eq!(h.text("a.txt").as_deref(), Some("agent"));
    assert_eq!(h.text("b.txt").as_deref(), Some("person"));
    assert_eq!(h.direct.note(&h.session), None);
    assert_eq!(h.by().await, by(&[("a.txt", Agent), ("b.txt", Both)]));
    let again = h.direct.undo_go_back(&h.session).await;
    assert_eq!(again.map_err(|e| e.code), Err(ErrorCode::Conflict));
}

#[tokio::test]
async fn the_next_turn_ends_what_can_be_undone() {
    let h = Harness::start(&[]).await;
    h.turn(1).await;
    h.agent_runs("t1", |h| h.put("a.txt", "agent")).await;
    h.turn_end().await;
    h.go_back(1).await;
    h.turn(2).await;
    assert_eq!(h.direct.note(&h.session), None);
    h.turn_end().await;
    let undone = h.direct.undo_go_back(&h.session).await;
    assert_eq!(undone.map_err(|e| e.code), Err(ErrorCode::Conflict));
    assert_eq!(h.text("a.txt"), None);
}

#[tokio::test]
async fn going_back_is_refused_while_a_turn_or_a_tool_runs() {
    let h = Harness::start(&[]).await;
    h.turn(1).await;
    let refused = h.direct.go_back(&h.session, 1).await;
    assert_eq!(refused.map_err(|e| e.code), Err(ErrorCode::Conflict));
    // A tool still running after the turn's result, say a stray one.
    h.started("t1", "Bash", json!({"command": "make"})).await;
    h.turn_end().await;
    let refused = h.direct.go_back(&h.session, 1).await;
    assert_eq!(refused.map_err(|e| e.code), Err(ErrorCode::Conflict));
    h.ended("t1").await;
    assert_eq!(h.go_back(1).await, (vec![], vec![]));
}

#[tokio::test]
async fn the_101st_turn_drops_the_first() {
    let h = Harness::start(&[]).await;
    for turn in 1..=101 {
        h.turn(turn).await;
        h.turn_end().await;
    }
    let turns = h.turns().await;
    assert_eq!(turns.len(), 100);
    assert_eq!((turns[0], turns[99]), (101, 2));
    let dropped = h.direct.go_back(&h.session, 1).await;
    let dropped = dropped.err().unwrap_or_else(|| panic!("went back"));
    assert_eq!(dropped.code, ErrorCode::NotFound);
    assert!(dropped.message.contains("turn 2"), "{}", dropped.message);
}

impl Harness {
    /// A new direct mode over the same files and store, as after Ovra
    /// restarted: no agent runs.
    fn restart(&self) -> Harness {
        let direct = Direct::new(self.trees.clone(), self.store.clone());
        direct.keep_only(|_| true);
        Harness {
            trees: self.trees.clone(),
            store: self.store.clone(),
            direct,
            project: self.project.clone(),
            session: self.session.clone(),
        }
    }
}

#[tokio::test]
async fn checkpoints_survive_a_restart_and_a_change_made_meanwhile_is_kept() {
    let h = Harness::start(&[("b.txt", "base")]).await;
    h.turn(1).await;
    h.agent_runs("t1", |h| h.put("a.txt", "agent")).await;
    h.turn_end().await;
    h.turn(2).await;
    h.agent_runs("t2", |h| h.put("b.txt", "agent")).await;
    h.turn_end().await;
    // The person edits b.txt while Ovra is closed.
    h.put("b.txt", "agent + mine");

    let h = h.restart();
    assert_eq!(h.turns().await, [2, 1]);
    assert_eq!(
        h.go_back(1).await,
        (strings(&["a.txt"]), strings(&["b.txt"]))
    );
    assert_eq!(h.text("a.txt"), None);
    assert_eq!(h.text("b.txt").as_deref(), Some("agent + mine"));
    assert!(h.direct.can_undo_go_back(&h.session));
    assert!(h.direct.note(&h.session).is_some());

    // The go-back can still be undone after another restart.
    let h = h.restart();
    assert!(h.direct.can_undo_go_back(&h.session));
    let undone = h.direct.undo_go_back(&h.session).await;
    assert_eq!(undone.ok(), Some((strings(&["a.txt"]), vec![])));
    assert_eq!(h.text("a.txt").as_deref(), Some("agent"));
}

#[tokio::test]
async fn what_the_agent_did_after_the_last_save_is_kept_after_a_restart() {
    let h = Harness::start(&[]).await;
    h.turn(1).await;
    h.agent_runs("t1", |h| h.put("a.txt", "agent")).await;
    h.turn_end().await;
    h.turn(2).await;
    h.agent_runs("t2", |h| h.put("b.txt", "agent")).await;
    // Ovra quits before turn 2 ends: the last save was at its start.
    let h = h.restart();
    assert_eq!(h.turns().await, [2, 1]);
    assert_eq!(
        h.go_back(1).await,
        (strings(&["a.txt"]), strings(&["b.txt"]))
    );
    assert_eq!(h.text("b.txt").as_deref(), Some("agent"));
}

#[tokio::test]
async fn a_forgotten_session_has_no_checkpoints_after_a_restart() {
    let h = Harness::start(&[]).await;
    h.turn(1).await;
    h.agent_runs("t1", |h| h.put("a.txt", "agent")).await;
    h.turn_end().await;
    let restarted = Direct::new(h.trees.clone(), h.store.clone());
    restarted.keep_only(|_| false);
    assert!(
        restarted
            .checkpoints(&h.session)
            .await
            .is_ok_and(|l| l.is_empty())
    );
    let refused = restarted.go_back(&h.session, 1).await;
    let refused = refused.err().unwrap_or_else(|| panic!("went back"));
    assert!(
        refused.message.contains("nothing was restored"),
        "{}",
        refused.message
    );
    assert_eq!(h.text("a.txt").as_deref(), Some("agent"));
    // Nor after the next one.
    let again = Direct::new(h.trees.clone(), h.store.clone());
    assert!(
        again
            .checkpoints(&h.session)
            .await
            .is_ok_and(|l| l.is_empty())
    );
}

#[tokio::test]
async fn an_unreadable_save_keeps_nothing() {
    let h = Harness::start(&[]).await;
    h.turn(1).await;
    h.agent_runs("t1", |h| h.put("a.txt", "agent")).await;
    h.turn_end().await;
    let _ = crate::services::SessionStore::save_direct(&*h.store, "{not json");
    let h = h.restart();
    assert!(
        h.direct
            .checkpoints(&h.session)
            .await
            .is_ok_and(|l| l.is_empty())
    );
    assert!(h.direct.go_back(&h.session, 1).await.is_err());
    assert_eq!(h.text("a.txt").as_deref(), Some("agent"));
}

#[tokio::test]
async fn a_decided_review_lets_its_checkpoints_go() {
    let h = Harness::start(&[]).await;
    h.turn(1).await;
    h.agent_runs("t1", |h| h.put("a.txt", "agent")).await;
    h.turn_end().await;
    let approved = h.direct.approve(&h.session, None, &Guard::default()).await;
    assert!(approved.is_ok(), "{approved:?}");
    assert_eq!(h.turns().await, Vec::<u32>::new());
    h.turn(2).await;
    h.turn_end().await;
    assert_eq!(h.turns().await, [2]);
}

#[tokio::test]
async fn a_session_without_a_baseline_has_no_checkpoints() {
    let trees = Arc::new(Trees::default());
    *lock(&trees.fail_snapshots) = true;
    let direct = Direct::new(trees.clone(), no_store());
    let (project, s) = (ProjectId("p1".into()), session("s1"));
    assert!(direct.begin(&project, &s, DIR.into()).await.is_err());
    assert!(direct.turn_started(&s, 1, "one".into()));
    let refused = direct.checkpoints(&s).await.map_err(|e| e.code);
    assert_eq!(refused, Err(ErrorCode::Unavailable));
    let refused = direct.go_back(&s, 1).await.err();
    assert!(refused.is_some_and(|e| e.code == ErrorCode::Unavailable && e.message.contains("git")),);
}
