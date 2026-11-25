//! Direct mode against in-memory worktrees. The person's edits are made
//! with `put`/`delete` between or during the agent's tool calls.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{Value, json};

use super::*;
use crate::fakes::MemoryWorktrees;
use crate::orchestration::review::Guard;
use crate::protocol::orchestration::DiskUsage;
use crate::protocol::review::ChangeKind;
use crate::services::{
    BoxFuture, CommitAuthor, GitFiles, Included, IncludedFile, Picked, WorktreeInfo,
};

const DIR: &str = crate::test_path!("/p");

/// [`MemoryWorktrees`] plus renames, failing snapshots and paths that
/// `restore` silently skips, like the real one does for unsafe paths.
#[derive(Debug, Default)]
struct Trees {
    mem: MemoryWorktrees,
    /// `(from, to)`: a deletion of `from` and an addition of `to` in one
    /// diff are reported as one rename.
    renames: Mutex<Vec<(String, String)>>,
    /// Makes snapshots and looks fail.
    fail_snapshots: Mutex<bool>,
    skip_restore: Mutex<BTreeSet<String>>,
}

impl std::ops::Deref for Trees {
    type Target = MemoryWorktrees;
    fn deref(&self) -> &MemoryWorktrees {
        &self.mem
    }
}

impl Trees {
    /// Turns the deletes and adds of the renames set up into renames.
    fn renamed(&self, mut changes: Vec<Delta>) -> Vec<Delta> {
        for (from, to) in lock(&self.renames).iter() {
            let deleted = changes
                .iter()
                .position(|d| &d.path == from && d.kind == ChangeKind::Deleted);
            let added = changes
                .iter()
                .position(|d| &d.path == to && d.kind == ChangeKind::Added);
            if let (Some(deleted), Some(added)) = (deleted, added) {
                changes[added].kind = ChangeKind::Renamed;
                changes[added].from = Some(from.clone());
                changes.remove(deleted);
            }
        }
        changes
    }
}

impl Worktrees for Trees {
    fn snapshot(&self, dir: PathBuf) -> BoxFuture<'_, Result<String>> {
        Box::pin(async move {
            if *lock(&self.fail_snapshots) {
                return Err(conflict("snapshot failed"));
            }
            self.mem.snapshot(dir).await
        })
    }
    fn create(
        &self,
        root: PathBuf,
        dir: PathBuf,
        branch: String,
        base: String,
    ) -> BoxFuture<'_, Result<WorktreeInfo>> {
        self.mem.create(root, dir, branch, base)
    }
    fn changes(&self, dir: PathBuf, base: String) -> BoxFuture<'_, Result<Vec<Delta>>> {
        Box::pin(async move { Ok(self.renamed(self.mem.changes(dir, base).await?)) })
    }
    fn patch(
        &self,
        dir: PathBuf,
        base: String,
        paths: Vec<RelPath>,
    ) -> BoxFuture<'_, Result<String>> {
        self.mem.patch(dir, base, paths)
    }
    fn restore(
        &self,
        dir: PathBuf,
        base: String,
        paths: Vec<RelPath>,
    ) -> BoxFuture<'_, Result<()>> {
        let skip = lock(&self.skip_restore).clone();
        let paths = paths
            .into_iter()
            .filter(|p| !skip.contains(p.as_str()))
            .collect();
        self.mem.restore(dir, base, paths)
    }
    fn apply(
        &self,
        tree: String,
        base: String,
        into: PathBuf,
        paths: Vec<RelPath>,
    ) -> BoxFuture<'_, Result<Vec<String>>> {
        self.mem.apply(tree, base, into, paths)
    }
    fn commit(
        &self,
        dir: PathBuf,
        paths: Vec<RelPath>,
        message: String,
    ) -> BoxFuture<'_, Result<String>> {
        self.mem.commit(dir, paths, message)
    }
    fn remove(
        &self,
        root: PathBuf,
        dir: PathBuf,
        included: Vec<IncludedFile>,
    ) -> BoxFuture<'_, Result<()>> {
        self.mem.remove(root, dir, included)
    }
    fn include(&self, root: PathBuf, dir: PathBuf) -> BoxFuture<'_, Result<Included>> {
        self.mem.include(root, dir)
    }
    fn pick(
        &self,
        root: PathBuf,
        base: String,
        start: String,
        reviewed: String,
        message: String,
        author: Option<CommitAuthor>,
    ) -> BoxFuture<'_, Result<Picked>> {
        self.mem.pick(root, base, start, reviewed, message, author)
    }
    fn usage(&self, dir: PathBuf, budget: Duration) -> BoxFuture<'_, Result<DiskUsage>> {
        self.mem.usage(dir, budget)
    }
    fn lock(&self, root: PathBuf, dir: PathBuf, reason: String) -> BoxFuture<'_, Result<()>> {
        self.mem.lock(root, dir, reason)
    }
    fn unlock(&self, root: PathBuf, dir: PathBuf) -> BoxFuture<'_, Result<()>> {
        self.mem.unlock(root, dir)
    }
    fn prune_snapshots(&self, root: PathBuf, keep: Vec<String>) -> BoxFuture<'_, Result<()>> {
        self.mem.prune_snapshots(root, keep)
    }
    fn git_files(&self, dir: PathBuf) -> BoxFuture<'_, Result<GitFiles>> {
        self.mem.git_files(dir)
    }
    fn look(&self, dir: PathBuf) -> BoxFuture<'_, Result<String>> {
        Box::pin(async move {
            if *lock(&self.fail_snapshots) {
                return Err(conflict("look failed"));
            }
            self.mem.look(dir).await
        })
    }
    fn patches(
        &self,
        dir: PathBuf,
        from: String,
        to: String,
        paths: Vec<RelPath>,
    ) -> BoxFuture<'_, Result<std::collections::BTreeMap<String, String>>> {
        self.mem.patches(dir, from, to, paths)
    }
    fn diff(&self, dir: PathBuf, from: String, to: String) -> BoxFuture<'_, Result<Vec<Delta>>> {
        Box::pin(async move { Ok(self.renamed(self.mem.diff(dir, from, to).await?)) })
    }
    fn take(
        &self,
        tree: String,
        into: PathBuf,
        paths: Vec<RelPath>,
    ) -> BoxFuture<'_, Result<crate::services::Taken>> {
        self.mem.take(tree, into, paths)
    }
    fn delete_branch(
        &self,
        root: PathBuf,
        branch: String,
        base: String,
    ) -> BoxFuture<'_, Result<()>> {
        self.mem.delete_branch(root, branch, base)
    }
}

struct Harness {
    trees: Arc<Trees>,
    direct: Direct,
    project: ProjectId,
    session: SessionId,
}

fn session(id: &str) -> SessionId {
    SessionId(id.into())
}

fn write_input(path: &str) -> Value {
    json!({"file_path": format!("{DIR}/{path}"), "content": "x"})
}

impl Harness {
    /// A project whose checkout holds `files` when the session starts.
    async fn start(files: &[(&str, &str)]) -> Self {
        let trees = Trees::default();
        trees.mem.put(DIR, ".keep", "");
        for (path, text) in files {
            trees.mem.put(DIR, path, text);
        }
        let trees = Arc::new(trees);
        let h = Self {
            direct: Direct::new(trees.clone()),
            trees,
            project: ProjectId("p1".into()),
            session: session("s1"),
        };
        let baseline = h.direct.begin(&h.project, &h.session, DIR.into()).await;
        assert!(baseline.is_ok(), "{baseline:?}");
        h
    }

    fn mem(&self) -> &Trees {
        &self.trees
    }

    /// The person (or anything but the agent) changes a file.
    fn put(&self, path: &str, text: &str) {
        self.mem().put(DIR, path, text);
    }

    fn delete(&self, path: &str) {
        self.mem().delete(DIR, path);
    }

    fn text(&self, path: &str) -> Option<String> {
        self.mem().files(DIR).get(path).cloned()
    }

    async fn started(&self, id: &str, tool: &str, input: Value) {
        self.direct
            .tool_started(&self.session, id, tool, &input)
            .await;
    }

    async fn ended(&self, id: &str) {
        assert!(self.direct.tool_ended(&self.session, id));
        self.direct.catch_up(&self.session).await;
    }

    /// The agent writes one file with a file tool.
    async fn agent_writes(&self, id: &str, path: &str, text: &str) {
        self.started(id, "Write", write_input(path)).await;
        self.put(path, text);
        self.ended(id).await;
    }

    async fn by(&self) -> Vec<(String, ChangedBy)> {
        let files = self.direct.files(&self.session).await;
        files
            .unwrap_or_else(|e| panic!("{e:?}"))
            .into_iter()
            .map(|f| (f.path, f.by))
            .collect()
    }

    async fn reject(&self) -> Vec<String> {
        let outcome = self.direct.reject(&self.session).await;
        outcome.unwrap_or_else(|e| panic!("{e:?}")).conflicts
    }
}

fn by(list: &[(&str, ChangedBy)]) -> Vec<(String, ChangedBy)> {
    list.iter().map(|(p, b)| ((*p).to_owned(), *b)).collect()
}

use ChangedBy::{Agent, Both, Outside};

#[tokio::test]
async fn one_running_direct_writer_per_project_and_a_resumed_session_keeps_its_baseline() {
    let h = Harness::start(&[]).await;
    let s2 = session("s2");
    let other = h.direct.begin(&h.project, &s2, DIR.into()).await;
    assert_eq!(other.err().map(|e| e.code), Some(ErrorCode::Conflict));
    // Another project is independent.
    h.mem().put("/q", "a", "");
    let elsewhere = h
        .direct
        .begin(&ProjectId("p2".into()), &session("s3"), "/q".into())
        .await;
    assert!(elsewhere.is_ok());

    h.agent_writes("t1", "a.rs", "agent\n").await;
    h.direct.session_ended(&h.session);
    assert_eq!(h.direct.writer(&h.project), None, "ended: the lock is free");
    // A resumed session (or a review comment) keeps its baseline.
    let again = h.direct.begin(&h.project, &h.session, DIR.into()).await;
    assert_eq!(again.ok().as_deref(), Some("c1"), "same baseline");
    assert_eq!(h.direct.writer(&h.project), Some(h.session.clone()));
    assert_eq!(h.by().await, by(&[("a.rs", Agent)]));
    h.direct.session_ended(&h.session);

    // The next session may start while the first one waits for review.
    assert!(h.direct.begin(&h.project, &s2, DIR.into()).await.is_ok());
    let resumed = h.direct.begin(&h.project, &h.session, DIR.into()).await;
    assert_eq!(resumed.err().map(|e| e.code), Some(ErrorCode::Conflict));
    assert!(
        h.direct
            .approve(&h.session, None, &Guard::default())
            .await
            .is_ok()
    );
    let reviewed = h.direct.files(&h.session).await;
    assert_eq!(reviewed.err().map(|e| e.code), Some(ErrorCode::Conflict));
}

#[tokio::test]
async fn to_an_earlier_session_a_later_ones_changes_are_outside() {
    let h = Harness::start(&[("a.rs", "a\n")]).await;
    h.agent_writes("t1", "a.rs", "first\n").await;
    h.direct.session_ended(&h.session);
    let s2 = session("s2");
    assert!(h.direct.begin(&h.project, &s2, DIR.into()).await.is_ok());
    h.direct
        .tool_started(&s2, "t2", "Write", &write_input("a.rs"))
        .await;
    h.put("a.rs", "first + second\n");
    h.direct
        .tool_started(&s2, "t3", "Write", &write_input("b.rs"))
        .await;
    h.put("b.rs", "second\n");
    assert!(h.direct.tool_ended(&s2, "t2") && h.direct.tool_ended(&s2, "t3"));
    let later = h.direct.files(&s2).await.unwrap_or_default();
    let later: Vec<_> = later.iter().map(|f| (f.path.as_str(), f.by)).collect();
    assert_eq!(later, [("a.rs", Agent), ("b.rs", Agent)]);

    assert_eq!(h.by().await, by(&[("a.rs", Both), ("b.rs", Outside)]));
    assert_eq!(h.reject().await, ["a.rs"]);
    assert_eq!(h.text("a.rs").as_deref(), Some("first + second\n"));
    assert_eq!(h.text("b.rs").as_deref(), Some("second\n"));
}

#[tokio::test]
async fn a_failed_baseline_holds_the_lock_untracked_until_the_session_ends() {
    let direct = Direct::new(Arc::new(Trees::default()));
    let project = ProjectId("p1".into());
    // No working tree at /p: the snapshot fails.
    let first = direct.begin(&project, &session("s1"), DIR.into()).await;
    assert!(first.is_err());
    assert_eq!(direct.writer(&project), Some(session("s1")));
    let files = direct.files(&session("s1")).await;
    assert_eq!(files.err().map(|e| e.code), Some(ErrorCode::Conflict));
    direct.session_ended(&session("s1"));
    assert_eq!(direct.writer(&project), None);
}

#[tokio::test]
async fn reject_reverts_only_the_agents_files_and_keeps_the_persons() {
    let h = Harness::start(&[("a.rs", "a\n"), ("b.rs", "b\n"), ("gone.rs", "g\n")]).await;
    h.agent_writes("t1", "a.rs", "agent\n").await;
    h.agent_writes("t2", "new.rs", "agent\n").await;
    h.started("t3", "Bash", json!({"command": "rm gone.rs"}))
        .await;
    h.delete("gone.rs");
    h.ended("t3").await;
    // Between tool calls only the person edits.
    h.put("b.rs", "mine\n");
    h.put("notes.md", "mine\n");

    assert_eq!(
        h.by().await,
        by(&[
            ("a.rs", Agent),
            ("b.rs", Outside),
            ("gone.rs", Agent),
            ("new.rs", Agent),
            ("notes.md", Outside),
        ])
    );
    assert_eq!(h.reject().await, Vec::<String>::new());
    assert_eq!(h.text("a.rs").as_deref(), Some("a\n"));
    assert_eq!(h.text("gone.rs").as_deref(), Some("g\n"));
    assert_eq!(h.text("new.rs"), None);
    assert_eq!(h.text("b.rs").as_deref(), Some("mine\n"));
    assert_eq!(h.text("notes.md").as_deref(), Some("mine\n"));
    // The agent still runs: it starts over from the tree as it is now.
    assert_eq!(h.direct.writer(&h.project), Some(h.session.clone()));
    assert!(h.by().await.is_empty());
}

#[tokio::test]
async fn uncommitted_work_from_before_the_session_is_the_baseline() {
    // The person's uncommitted edit is in the checkout when the agent starts.
    let h = Harness::start(&[("wip.rs", "half done\n")]).await;
    h.started("t1", "Bash", json!({"command": "rm wip.rs"}))
        .await;
    h.delete("wip.rs");
    h.ended("t1").await;
    assert_eq!(h.reject().await, Vec::<String>::new());
    assert_eq!(h.text("wip.rs").as_deref(), Some("half done\n"));
}

#[tokio::test]
async fn a_file_the_person_edited_before_the_agent_is_never_reverted() {
    let h = Harness::start(&[("a.rs", "a\n")]).await;
    h.put("a.rs", "mine\n");
    h.agent_writes("t1", "a.rs", "mine + agent\n").await;
    assert_eq!(h.by().await, by(&[("a.rs", Both)]));
    assert_eq!(h.reject().await, ["a.rs"]);
    assert_eq!(h.text("a.rs").as_deref(), Some("mine + agent\n"));
}

#[tokio::test]
async fn a_file_the_person_edited_after_the_agent_is_never_reverted() {
    let h = Harness::start(&[("a.rs", "a\n"), ("b.rs", "b\n")]).await;
    h.agent_writes("t1", "a.rs", "agent\n").await;
    h.agent_writes("t2", "b.rs", "agent\n").await;
    // After the agent's last tool call, in the editor.
    h.put("a.rs", "agent + mine\n");
    assert_eq!(h.by().await, by(&[("a.rs", Both), ("b.rs", Agent)]));
    assert_eq!(h.reject().await, ["a.rs"]);
    assert_eq!(h.text("a.rs").as_deref(), Some("agent + mine\n"));
    assert_eq!(h.text("b.rs").as_deref(), Some("b\n"));
}

#[tokio::test]
async fn a_file_the_person_edited_between_two_agent_writes_is_never_reverted() {
    let h = Harness::start(&[("a.rs", "a\n")]).await;
    h.agent_writes("t1", "a.rs", "agent\n").await;
    h.put("a.rs", "agent + mine\n");
    h.agent_writes("t2", "a.rs", "agent + mine + agent\n").await;
    assert_eq!(h.by().await, by(&[("a.rs", Both)]));
    assert_eq!(h.reject().await, ["a.rs"]);
    assert_eq!(h.text("a.rs").as_deref(), Some("agent + mine + agent\n"));
}

#[tokio::test]
async fn a_file_the_person_recreated_after_the_agent_deleted_it_is_kept() {
    let h = Harness::start(&[("a.rs", "a\n")]).await;
    h.started("t1", "Bash", json!({"command": "rm a.rs"})).await;
    h.delete("a.rs");
    h.ended("t1").await;
    h.put("a.rs", "mine\n");
    assert_eq!(h.by().await, by(&[("a.rs", Both)]));
    assert_eq!(h.reject().await, ["a.rs"]);
    assert_eq!(h.text("a.rs").as_deref(), Some("mine\n"));
}

#[tokio::test]
async fn the_person_editing_during_a_file_tool_is_outside() {
    let h = Harness::start(&[("a.rs", "a\n"), ("b.rs", "b\n")]).await;
    h.started("t1", "Edit", json!({"file_path": "a.rs"})).await;
    h.put("a.rs", "agent\n");
    h.put("b.rs", "mine\n");
    h.ended("t1").await;
    assert_eq!(h.by().await, by(&[("a.rs", Agent), ("b.rs", Outside)]));
    assert!(h.reject().await.is_empty());
    assert_eq!(h.text("b.rs").as_deref(), Some("mine\n"));
}

#[tokio::test]
async fn a_tool_waiting_for_approval_does_not_claim_what_the_person_edits_meanwhile() {
    let h = Harness::start(&[("a.rs", "a\n")]).await;
    let input = json!({"command": "cargo fmt"});
    h.started("t1", "Bash", input.clone()).await;
    h.direct.tool_waiting(&h.session, "Bash", &input).await;
    // The approval card is open; the person keeps working.
    h.put("a.rs", "mine\n");
    h.direct
        .tool_answered(&h.session, "Bash", &input, true)
        .await;
    h.put("fmt.rs", "formatted\n");
    h.ended("t1").await;
    assert_eq!(h.by().await, by(&[("a.rs", Outside), ("fmt.rs", Agent)]));

    // A denied tool never runs.
    let rm = json!({"command": "rm a.rs"});
    h.started("t2", "Bash", rm.clone()).await;
    h.direct.tool_waiting(&h.session, "Bash", &rm).await;
    h.direct.tool_answered(&h.session, "Bash", &rm, false).await;
    h.put("later.rs", "mine\n");
    h.ended("t2").await;
    assert_eq!(
        h.by().await,
        by(&[("a.rs", Outside), ("fmt.rs", Agent), ("later.rs", Outside)])
    );
    assert!(h.reject().await.is_empty());
    assert_eq!(h.text("a.rs").as_deref(), Some("mine\n"));
    assert_eq!(h.text("fmt.rs"), None);
}

#[tokio::test]
async fn overlapping_tools_share_one_interval() {
    let h = Harness::start(&[]).await;
    h.started("t1", "Write", write_input("one.rs")).await;
    h.started("t2", "Write", write_input("two.rs")).await;
    h.put("one.rs", "1\n");
    h.ended("t1").await;
    // t2 still runs: its file must not look changed outside.
    h.put("two.rs", "2\n");
    h.ended("t2").await;
    assert_eq!(h.by().await, by(&[("one.rs", Agent), ("two.rs", Agent)]));
    let refused = h.direct.reject(&h.session).await;
    assert!(refused.is_ok());
}

#[tokio::test]
async fn a_tool_result_is_applied_before_the_next_tool_starts() {
    let h = Harness::start(&[("a.rs", "a\n")]).await;
    h.started("t1", "Write", write_input("one.rs")).await;
    h.put("one.rs", "1\n");
    // The result is reported, but the follow-up has not run yet.
    assert!(h.direct.tool_ended(&h.session, "t1"));
    h.put("a.rs", "mine\n");
    h.started("t2", "Write", write_input("two.rs")).await;
    h.put("two.rs", "2\n");
    h.ended("t2").await;
    assert_eq!(
        h.by().await,
        by(&[("a.rs", Outside), ("one.rs", Agent), ("two.rs", Agent)])
    );
    assert!(!h.direct.tool_ended(&session("other"), "t3"));
}

#[tokio::test]
async fn reject_waits_for_running_tools() {
    let h = Harness::start(&[]).await;
    h.started("t1", "Bash", json!({"command": "make"})).await;
    let refused = h.direct.reject(&h.session).await;
    assert_eq!(refused.err().map(|e| e.code), Some(ErrorCode::Conflict));
    let refused = h.direct.approve(&h.session, None, &Guard::default()).await;
    assert_eq!(refused.err().map(|e| e.code), Some(ErrorCode::Conflict));
    // Ending the session drops its tools: what they changed is outside.
    h.put("out.o", "bin\n");
    h.direct.session_ended(&h.session);
    assert_eq!(h.by().await, by(&[("out.o", Outside)]));
    assert_eq!(h.reject().await, Vec::<String>::new());
    assert_eq!(h.text("out.o").as_deref(), Some("bin\n"));
}

#[tokio::test]
async fn renames_revert_both_paths_unless_the_person_was_involved() {
    let h = Harness::start(&[("a.rs", "a\n"), ("x.rs", "x\n")]).await;
    lock(&h.mem().renames).push(("a.rs".into(), "b.rs".into()));
    lock(&h.mem().renames).push(("x.rs".into(), "y.rs".into()));
    h.started("t1", "Bash", json!({"command": "git mv a.rs b.rs"}))
        .await;
    h.delete("a.rs");
    h.put("b.rs", "a\n");
    h.ended("t1").await;
    // The person deletes x.rs; the agent writes a similar y.rs, and the
    // diff pairs them as a rename.
    h.delete("x.rs");
    h.agent_writes("t2", "y.rs", "x\n").await;
    let files = h.direct.files(&h.session).await.unwrap_or_default();
    let renames: Vec<_> = files
        .iter()
        .map(|f| (f.path.as_str(), f.from.as_deref(), f.by))
        .collect();
    assert_eq!(
        renames,
        [("b.rs", Some("a.rs"), Agent), ("y.rs", Some("x.rs"), Both)]
    );
    assert_eq!(h.reject().await, ["y.rs"]);
    assert_eq!(h.text("a.rs").as_deref(), Some("a\n"));
    assert_eq!(h.text("b.rs"), None);
    assert_eq!(h.text("x.rs"), None, "the person's delete stays");
    assert_eq!(h.text("y.rs").as_deref(), Some("x\n"));
}

#[tokio::test]
async fn undo_puts_back_what_reject_reverted_unless_it_changed_since() {
    let h = Harness::start(&[("a.rs", "a\n"), ("b.rs", "b\n")]).await;
    h.agent_writes("t1", "a.rs", "agent\n").await;
    h.agent_writes("t2", "b.rs", "agent\n").await;
    h.direct.session_ended(&h.session);
    assert!(h.reject().await.is_empty());
    assert!(h.direct.can_undo(&h.session));
    // The person edits b.rs after the reject: undo must not overwrite it.
    h.put("b.rs", "mine\n");
    let undone = h.direct.undo_reject(&h.session).await;
    let undone = undone.unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(undone.restored, ["a.rs"]);
    assert_eq!(undone.conflicts, ["b.rs"]);
    assert_eq!(undone.base.as_deref(), Some("c1"), "the old baseline again");
    assert_eq!(h.text("a.rs").as_deref(), Some("agent\n"));
    assert_eq!(h.text("b.rs").as_deref(), Some("mine\n"));
    assert!(!h.direct.can_undo(&h.session));
    // Attribution is back: b.rs is now changed by both.
    assert_eq!(h.by().await, by(&[("a.rs", Agent), ("b.rs", Both)]));
    let again = h.direct.undo_reject(&h.session).await;
    assert_eq!(again.err().map(|e| e.code), Some(ErrorCode::Conflict));
}

#[tokio::test]
async fn a_person_save_during_a_shell_call_makes_the_file_both() {
    let h = Harness::start(&[("a.rs", "a\n")]).await;
    h.started("t1", "Bash", json!({"command": "make"})).await;
    h.put("a.rs", "mine\n");
    h.direct.person_saved(&h.project, "a.rs");
    h.put("out.o", "built\n");
    h.ended("t1").await;
    // A save between tool calls is just outside.
    h.put("c.rs", "mine\n");
    h.direct.person_saved(&h.project, "c.rs");
    assert_eq!(
        h.by().await,
        by(&[("a.rs", Both), ("c.rs", Outside), ("out.o", Agent)])
    );
    assert_eq!(h.reject().await, ["a.rs"]);
    assert_eq!(h.text("a.rs").as_deref(), Some("mine\n"));
}

#[tokio::test]
async fn a_folder_the_person_renames_during_a_shell_call_is_both() {
    let h = Harness::start(&[("src/a.rs", "a\n"), ("srcx/b.rs", "b\n")]).await;
    h.started("t1", "Bash", json!({"command": "make"})).await;
    h.delete("src/a.rs");
    h.put("lib/a.rs", "a\n");
    h.direct.person_saved(&h.project, "src");
    h.direct.person_saved(&h.project, "lib");
    // Only the folder's own files: `srcx` is not inside `src`.
    h.put("srcx/b.rs", "agent\n");
    h.ended("t1").await;
    assert_eq!(
        h.by().await,
        by(&[("lib/a.rs", Both), ("srcx/b.rs", Agent)])
    );
}

#[tokio::test]
async fn a_file_restore_skipped_is_reported_as_a_conflict() {
    let h = Harness::start(&[("a.rs", "a\n")]).await;
    h.agent_writes("t1", "a.rs", "agent\n").await;
    h.agent_writes("t2", "b.rs", "agent\n").await;
    lock(&h.mem().skip_restore).insert("b.rs".into());
    assert_eq!(h.reject().await, ["b.rs"]);
    assert_eq!(h.text("a.rs").as_deref(), Some("a\n"));
}

#[tokio::test]
async fn reject_keeps_a_snapshot_of_what_it_reverted() {
    let h = Harness::start(&[]).await;
    h.agent_writes("t1", "a.rs", "agent\n").await;
    let outcome = h.direct.reject(&h.session).await;
    let snapshot = outcome.ok().and_then(|o| o.snapshot).unwrap_or_default();
    let kept = h.mem().changes(DIR.into(), snapshot).await;
    let kept: Vec<_> = kept
        .unwrap_or_default()
        .into_iter()
        .map(|d| d.path)
        .collect();
    assert_eq!(kept, ["a.rs"], "the snapshot still has the agent's a.rs");
}

#[tokio::test]
async fn failed_snapshots_make_nothing_the_agents() {
    let h = Harness::start(&[("a.rs", "a\n")]).await;
    h.agent_writes("t1", "a.rs", "agent\n").await;
    *lock(&h.mem().fail_snapshots) = true;
    // The interval's own looks fail: neither its file nor the earlier
    // one can be vouched for.
    h.agent_writes("t2", "b.rs", "agent\n").await;
    *lock(&h.mem().fail_snapshots) = false;
    h.agent_writes("t3", "c.rs", "agent\n").await;
    assert_eq!(
        h.by().await,
        by(&[("a.rs", Both), ("b.rs", Outside), ("c.rs", Agent)])
    );
    // Reject also needs its safety snapshot.
    *lock(&h.mem().fail_snapshots) = true;
    let refused = h.direct.reject(&h.session).await;
    assert!(refused.is_err());
    assert_eq!(h.text("c.rs").as_deref(), Some("agent\n"));
}

#[tokio::test]
async fn file_tools_outside_the_project_and_other_tools_claim_nothing() {
    let h = Harness::start(&[]).await;
    h.started("t1", "Write", json!({"file_path": "/etc/x"}))
        .await;
    h.put("a.rs", "mine\n");
    h.ended("t1").await;
    h.started("t2", "Edit", json!({"file_path": "../p/b.rs"}))
        .await;
    h.put("b.rs", "mine\n");
    h.ended("t2").await;
    h.started("t3", "Read", json!({"file_path": "c.rs"})).await;
    h.put("c.rs", "mine\n");
    h.ended("t3").await;
    assert_eq!(
        h.by().await,
        by(&[("a.rs", Outside), ("b.rs", Outside), ("c.rs", Outside)])
    );
    assert!(h.reject().await.is_empty());
    assert_eq!(h.text("c.rs").as_deref(), Some("mine\n"));
}

#[test]
fn tool_paths_are_relative_to_the_project() {
    let dir = Path::new(crate::test_path!("/w/p"));
    assert_eq!(
        relative(dir, crate::test_path!("/w/p/src/a.rs")).as_deref(),
        Some("src/a.rs")
    );
    assert_eq!(
        relative(dir, "src/./x/../a.rs").as_deref(),
        Some("src/a.rs")
    );
    assert_eq!(relative(dir, crate::test_path!("/w/p/../q/a.rs")), None);
    assert_eq!(relative(dir, "../p/a.rs"), None, "lexical only");
    assert_eq!(relative(dir, crate::test_path!("/w/p")), None);
    assert_eq!(relative(dir, "/etc/passwd"), None);
}

#[tokio::test]
async fn approve_commits_only_the_agents_own_files() {
    let h = Harness::start(&[("a.rs", "a\n"), ("both.rs", "b\n")]).await;
    lock(&h.mem().renames).push(("a.rs".into(), "moved.rs".into()));
    h.started("t1", "Bash", json!({"command": "mv a.rs moved.rs"}))
        .await;
    h.delete("a.rs");
    h.put("moved.rs", "a\n");
    h.ended("t1").await;
    h.agent_writes("t2", "both.rs", "agent\n").await;
    h.put("both.rs", "agent + mine\n");
    h.put("mine.rs", "mine\n");
    let outcome = h
        .direct
        .approve(&h.session, Some("feat: move a".into()), &Guard::default())
        .await
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(outcome.conflicts, ["both.rs"]);
    assert!(outcome.snapshot.is_some());
    let committed = lock(&h.mem().committed).clone();
    assert_eq!(
        committed,
        [(
            PathBuf::from(DIR),
            vec!["moved.rs".to_owned(), "a.rs".to_owned()],
            "feat: move a".to_owned()
        )]
    );
    assert_eq!(h.text("both.rs").as_deref(), Some("agent + mine\n"));
    assert!(h.by().await.is_empty(), "starts over");
    h.direct.session_ended(&h.session);
    assert_eq!(h.direct.writer(&h.project), None);
}

#[tokio::test]
async fn a_session_without_agent_changes_is_forgotten_after_it_ends() {
    let h = Harness::start(&[]).await;
    h.put("mine.rs", "mine\n");
    h.direct.session_ended(&h.session);
    assert_eq!(h.direct.writer(&h.project), None);
    // Forgotten when the next session starts.
    assert!(
        h.direct
            .begin(&h.project, &session("s2"), DIR.into())
            .await
            .is_ok()
    );
    let gone = h.direct.files(&h.session).await;
    assert_eq!(gone.err().map(|e| e.code), Some(ErrorCode::Conflict));
}

mod turns;

/// Through the core: `agents.start` takes the lock and the baseline, and the
/// host's hooks feed the tracker.
mod through_the_core {
    use super::*;
    use crate::Core;
    use crate::fakes::MemoryProjects;
    use crate::protocol::agents::AgentStatus;
    use crate::protocol::permissions::PermissionsResponse;
    use crate::protocol::review::{ReviewResponse, ReviewState};
    use crate::protocol::{Request, Response};
    use crate::services::{AgentHost, AgentRuntime, AgentSpec, RunningAgent};

    /// A runtime the test drives by hand through the hosts it was given.
    #[derive(Debug, Default)]
    struct Manual {
        hosts: Mutex<Vec<AgentHost>>,
    }

    #[derive(Debug)]
    struct Quiet;

    impl RunningAgent for Quiet {
        fn send(&self, _: String) -> Result<()> {
            Ok(())
        }
        fn stop(&self) {}
    }

    impl AgentRuntime for Manual {
        fn start(&self, _: AgentSpec, host: AgentHost) -> Result<Box<dyn RunningAgent>> {
            lock(&self.hosts).push(host);
            Ok(Box::new(Quiet))
        }
    }

    async fn call(core: &Core, json: Value) -> Result<Response> {
        let request: Request = serde_json::from_value(json).unwrap_or_else(|e| panic!("{e}"));
        core.handle(request).await
    }

    fn start() -> Value {
        json!({"method": "agents.start", "params": {
            "project": "p1", "agent": "claude", "mode": "direct", "prompt": "go"}})
    }

    async fn pending(core: &Core) -> Vec<crate::protocol::permissions::PermissionId> {
        match call(core, json!({"method": "permissions.pending"})).await {
            Ok(Response::Permissions(PermissionsResponse::Pending(list))) => {
                list.into_iter().map(|r| r.id).collect()
            }
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn hooks_permissions_and_results_attribute_files() {
        let trees = Arc::new(Trees::default());
        trees.put(DIR, "a.rs", "a\n");
        let runtime = Arc::new(Manual::default());
        let audit = Arc::new(crate::fakes::MemoryAudit::default());
        let core = Core::builder("test")
            .projects(Arc::new(MemoryProjects::default()))
            .agents(runtime.clone())
            .worktrees(trees.clone())
            .audit(audit.clone())
            .build();
        let added = call(
            &core,
            json!({"method": "projects.add", "params": {"root": DIR}}),
        )
        .await;
        assert!(added.is_ok());
        assert!(call(&core, start()).await.is_ok());
        let second = call(&core, start()).await;
        assert_eq!(second.err().map(|e| e.code), Some(ErrorCode::Conflict));
        let host = lock(&runtime.hosts)[0].clone();
        let s1 = host.session().clone();

        let input = json!({"command": "cargo fmt"});
        let hook = json!({
            "hook_event_name": "PreToolUse", "tool_use_id": "t1",
            "tool_name": "Bash", "tool_input": input,
        });
        assert_eq!(
            host.before_tool(&hook).await,
            crate::agents::BeforeTool::Proceed
        );
        let asking = tokio::spawn({
            let host = host.clone();
            async move { host.ask("Bash".into(), input, None, None).await }
        });
        let id = loop {
            if let Some(id) = pending(&core).await.pop() {
                break id;
            }
            tokio::task::yield_now().await;
        };
        // The person edits while the approval card is open.
        trees.put(DIR, "a.rs", "mine\n");
        let answered = call(
            &core,
            json!({"method": "permissions.answer", "params": {"id": id, "decision": "allow"}}),
        )
        .await;
        assert!(answered.is_ok(), "{answered:?}");
        assert!(asking.await.is_ok());
        trees.put(DIR, "fmt.rs", "formatted\n");
        host.message(crate::protocol::agents::AgentMessage::ToolResult {
            tool_use_id: "t1".into(),
            output: String::new(),
            is_error: false,
            parent: None,
        });
        host.tools_settled().await;
        trees.put(DIR, "later.rs", "mine\n");

        let direct = &core.inner_sessions().direct;
        let files = direct.files(&s1).await.unwrap_or_default();
        let by: Vec<_> = files.iter().map(|f| (f.path.as_str(), f.by)).collect();
        assert_eq!(
            by,
            [("a.rs", Outside), ("fmt.rs", Agent), ("later.rs", Outside)]
        );

        // Ending the session frees the lock; its review stays.
        host.status(AgentStatus::Stopped);
        assert_eq!(direct.writer(&ProjectId("p1".into())), None);
        assert!(call(&core, start()).await.is_ok());
        let list = call(
            &core,
            json!({"method": "review.list", "params": {"project": "p1"}}),
        );
        let Ok(Response::Review(ReviewResponse::List(items))) = list.await else {
            panic!("no review list");
        };
        let item = items.iter().find(|i| i.session == s1).cloned();
        let item = item.unwrap_or_else(|| panic!("no item for s1 in {items:?}"));
        assert_eq!(item.files.len(), 3);

        let done = call(
            &core,
            json!({"method": "review.reject", "params": {"id": item.id}}),
        );
        let Ok(Response::Review(ReviewResponse::Done {
            item,
            conflicts,
            restored,
        })) = done.await
        else {
            panic!("reject failed");
        };
        assert!(conflicts.is_empty());
        assert_eq!(restored, ["fmt.rs"]);
        assert_eq!(item.state, ReviewState::Rejected);
        assert!(item.can_undo);
        // The decided item keeps the files it was rejected with.
        let by: Vec<_> = item.files.iter().map(|f| (f.path.as_str(), f.by)).collect();
        assert_eq!(
            by,
            [("a.rs", Outside), ("fmt.rs", Agent), ("later.rs", Outside)]
        );
        let files = trees.files(DIR);
        let left: Vec<_> = files
            .iter()
            .map(|(p, t)| (p.as_str(), t.as_str()))
            .collect();
        assert_eq!(left, [("a.rs", "mine\n"), ("later.rs", "mine\n")]);

        // Undo puts the agent's file back and reopens the item as it was.
        let undo = call(
            &core,
            json!({"method": "review.undo_reject", "params": {"id": item.id}}),
        );
        let Ok(Response::Review(ReviewResponse::Done {
            item,
            conflicts,
            restored,
        })) = undo.await
        else {
            panic!("undo failed");
        };
        assert!(conflicts.is_empty());
        assert_eq!(restored, ["fmt.rs"]);
        assert_eq!(item.state, ReviewState::Open);
        assert!(!item.can_undo);
        let by: Vec<_> = item.files.iter().map(|f| (f.path.as_str(), f.by)).collect();
        assert_eq!(
            by,
            [("a.rs", Outside), ("fmt.rs", Agent), ("later.rs", Outside)]
        );
        assert_eq!(
            trees.files(DIR).get("fmt.rs").map(String::as_str),
            Some("formatted\n")
        );
        // The person's decisions are in the audit log.
        use crate::protocol::audit::{AuditEvent, ReviewAction};
        use crate::services::AuditLog;
        let actions: Vec<_> = audit
            .list(&ProjectId("p1".into()), Default::default(), 100)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|e| match e.event {
                AuditEvent::ReviewDecided { action, .. } => Some((e.session, action)),
                _ => None,
            })
            .collect();
        assert_eq!(
            actions,
            [
                (Some(s1.clone()), ReviewAction::Rejected),
                (Some(s1), ReviewAction::UndoneReject)
            ]
        );
    }

    /// Writes files into the in-memory checkout, like the editor's save.
    #[derive(Debug)]
    struct EditorFs(Arc<Trees>);

    fn no<T>() -> crate::services::BoxFuture<'static, Result<T>> {
        Box::pin(async { Err(conflict("not in this test")) })
    }

    impl crate::services::FsService for EditorFs {
        fn list_dir(
            &self,
            _: PathBuf,
            _: RelPath,
        ) -> crate::services::BoxFuture<'_, Result<crate::protocol::fs::DirListing>> {
            no()
        }
        fn read_file(
            &self,
            _: PathBuf,
            _: RelPath,
        ) -> crate::services::BoxFuture<'_, Result<crate::protocol::fs::FileContent>> {
            no()
        }
        fn list_files(
            &self,
            _: PathBuf,
            _: u32,
            _: u32,
        ) -> crate::services::BoxFuture<'_, Result<crate::protocol::fs::FileList>> {
            no()
        }
        fn search(
            &self,
            _: crate::protocol::search::SearchId,
            _: PathBuf,
            _: crate::protocol::search::SearchQuery,
            _: crate::services::EventSink,
            _: crate::services::SearchCancel,
        ) -> Result<()> {
            Err(conflict("not in this test"))
        }
        fn write_file(
            &self,
            root: PathBuf,
            path: RelPath,
            text: String,
            _: Option<u64>,
        ) -> crate::services::BoxFuture<'_, Result<u64>> {
            self.0.put(root, path.as_str(), &text);
            Box::pin(async { Ok(1) })
        }
        fn create(
            &self,
            _: PathBuf,
            _: RelPath,
            _: bool,
        ) -> crate::services::BoxFuture<'_, Result<()>> {
            no()
        }
        fn rename(
            &self,
            _: PathBuf,
            _: RelPath,
            _: RelPath,
        ) -> crate::services::BoxFuture<'_, Result<()>> {
            no()
        }
        fn copy(
            &self,
            _: PathBuf,
            _: RelPath,
            _: RelPath,
        ) -> crate::services::BoxFuture<'_, Result<()>> {
            no()
        }
        fn trash(&self, _: PathBuf, _: Vec<RelPath>) -> crate::services::BoxFuture<'_, Result<()>> {
            no()
        }
        fn watch(&self, _: ProjectId, _: PathBuf, _: crate::services::EventSink) -> Result<()> {
            Ok(())
        }
        fn unwatch(&self, _: &ProjectId) {}
    }

    struct Fixture {
        trees: Arc<Trees>,
        core: Core,
        runtime: Arc<Manual>,
    }

    async fn fixture(files: &[(&str, &str)]) -> Fixture {
        let trees = Arc::new(Trees::default());
        for (path, text) in files {
            trees.put(DIR, path, text);
        }
        let runtime = Arc::new(Manual::default());
        let core = Core::builder("test")
            .projects(Arc::new(MemoryProjects::default()))
            .fs(Arc::new(EditorFs(trees.clone())))
            .agents(runtime.clone())
            .worktrees(trees.clone())
            .build();
        let added = call(
            &core,
            json!({"method": "projects.add", "params": {"root": DIR}}),
        )
        .await;
        assert!(added.is_ok());
        Fixture {
            trees,
            core,
            runtime,
        }
    }

    impl Fixture {
        async fn start(&self) -> AgentHost {
            assert!(call(&self.core, start()).await.is_ok());
            lock(&self.runtime.hosts)
                .last()
                .cloned()
                .unwrap_or_else(|| panic!("no host"))
        }

        async fn list(&self) -> Vec<crate::protocol::review::ReviewItem> {
            let list = call(
                &self.core,
                json!({"method": "review.list", "params": {"project": "p1"}}),
            );
            match list.await {
                Ok(Response::Review(ReviewResponse::List(items))) => items,
                other => panic!("{other:?}"),
            }
        }
    }

    async fn bash(host: &AgentHost, id: &str, during: impl std::future::Future<Output = ()>) {
        let hook = json!({
            "tool_use_id": id, "tool_name": "Bash", "tool_input": {"command": "cargo build"},
        });
        assert_eq!(
            host.before_tool(&hook).await,
            crate::agents::BeforeTool::Proceed
        );
        during.await;
        host.message(crate::protocol::agents::AgentMessage::ToolResult {
            tool_use_id: id.into(),
            output: String::new(),
            is_error: false,
            parent: None,
        });
        host.tools_settled().await;
    }

    /// UX round 4: the person saved a file in Ovra's editor while the
    /// agent's `Bash` call ran, and reject reverted it.
    #[tokio::test]
    async fn a_file_saved_in_the_editor_during_a_bash_call_is_never_reverted() {
        let f = fixture(&[("a.rs", "a\n"), ("b.rs", "b\n")]).await;
        let host = f.start().await;
        bash(&host, "t1", async {
            f.trees.put(DIR, "target.o", "built\n");
            let save = json!({"method": "fs.write_file", "params": {
                "project": "p1", "path": "a.rs", "text": "mine\n", "expected_mtime_ms": null}});
            assert!(call(&f.core, save).await.is_ok());
            // The agent's command also touches a file the person saves.
            f.trees.put(DIR, "b.rs", "agent\n");
            let save = json!({"method": "fs.write_file", "params": {
                "project": "p1", "path": "b.rs", "text": "agent + mine\n", "expected_mtime_ms": null}});
            assert!(call(&f.core, save).await.is_ok());
        })
        .await;
        host.status(AgentStatus::Stopped);
        let items = f.list().await;
        let by: Vec<_> = items[0]
            .files
            .iter()
            .map(|x| (x.path.as_str(), x.by))
            .collect();
        // The command may have touched them too, so they are both's.
        assert_eq!(by, [("a.rs", Both), ("b.rs", Both), ("target.o", Agent)]);

        let reject = json!({"method": "review.reject", "params": {"id": items[0].id}});
        let Ok(Response::Review(ReviewResponse::Done {
            conflicts,
            restored,
            ..
        })) = call(&f.core, reject).await
        else {
            panic!("reject failed");
        };
        assert_eq!(
            (conflicts, restored),
            (
                vec!["a.rs".to_owned(), "b.rs".to_owned()],
                vec!["target.o".to_owned()]
            )
        );
        let files = f.trees.files(DIR);
        assert_eq!(files.get("a.rs").map(String::as_str), Some("mine\n"));
        assert_eq!(
            files.get("b.rs").map(String::as_str),
            Some("agent + mine\n")
        );
        assert_eq!(files.get("target.o"), None);
    }

    /// UX round 4 (NEW-30): the second direct session's review overflowed
    /// the stack once the first one was forgotten.
    #[tokio::test]
    async fn reviews_of_consecutive_direct_sessions_answer() {
        let f = fixture(&[("a.rs", "a\n")]).await;
        let first = f.start().await;
        f.trees.put(DIR, "notes.md", "mine\n");
        first.status(AgentStatus::Stopped);
        let second = f.start().await;
        bash(&second, "t1", async {
            f.trees.put(DIR, "a.rs", "agent\n");
        })
        .await;
        second.status(AgentStatus::Stopped);
        let items = f.list().await;
        let shown: Vec<_> = items
            .iter()
            .map(|i| (i.session.0.as_str(), i.files.len()))
            .collect();
        // s2's baseline already has notes.md; s1, forgotten, shows both files.
        assert_eq!(shown, [("s2", 1), ("s1", 2)]);
        let agents = json!({"method": "agents.list", "params": {"project": "p1"}});
        assert!(call(&f.core, agents).await.is_ok());
        // The forgotten first session restores nothing, and says why.
        let reject = json!({"method": "review.reject", "params": {"id": items[1].id}});
        let refused = call(&f.core, reject).await;
        let message = refused.err().map(|e| e.message).unwrap_or_default();
        assert!(message.contains("nothing was restored"), "{message}");
    }

    /// `agents.go_back` and its undo, the session's fields, and the note
    /// the agent's next message starts with.
    #[tokio::test]
    async fn going_back_through_the_core_tells_the_agent() {
        use crate::protocol::agents::{AgentMessage, AgentsResponse, MessageFrom, SessionInfo};
        let f = fixture(&[("a.rs", "a\n")]).await;
        let host = f.start().await;
        bash(&host, "t1", async {
            f.trees.put(DIR, "gen.rs", "agent\n");
        })
        .await;
        let back = json!({"method": "agents.go_back", "params": {"session": "s1", "turn": 1}});
        let refused = call(&f.core, back.clone()).await;
        assert_eq!(refused.err().map(|e| e.code), Some(ErrorCode::Conflict));
        host.status(AgentStatus::Idle);
        let list = json!({"method": "agents.checkpoints", "params": {"session": "s1"}});
        let Ok(Response::Agents(AgentsResponse::Checkpoints(list))) = call(&f.core, list).await
        else {
            panic!("no checkpoints");
        };
        let list: Vec<_> = list.iter().map(|c| (c.turn, c.message.as_str())).collect();
        assert_eq!(list, [(1, "go")]);

        let went = call(&f.core, back).await;
        let Ok(Response::Agents(AgentsResponse::WentBack {
            restored,
            conflicts,
            can_undo: true,
        })) = went
        else {
            panic!("{went:?}");
        };
        assert_eq!((restored, conflicts), (vec!["gen.rs".to_owned()], vec![]));
        assert!(!f.trees.files(DIR).contains_key("gen.rs"));
        let info = || async {
            let list = json!({"method": "agents.list", "params": {"project": "p1"}});
            match call(&f.core, list).await {
                Ok(Response::Agents(AgentsResponse::List(list))) => list[0].clone(),
                other => panic!("{other:?}"),
            }
        };
        let fields = |i: SessionInfo| (i.undone_from, i.can_undo_go_back);
        assert_eq!(fields(info().await), (Some(1), true));

        let send = json!({"method": "agents.send", "params": {"session": "s1", "text": "again"}});
        assert!(call(&f.core, send).await.is_ok());
        let transcript = json!({"method": "agents.transcript", "params": {"session": "s1"}});
        let Ok(Response::Agents(AgentsResponse::Transcript(messages))) =
            call(&f.core, transcript).await
        else {
            panic!("no transcript");
        };
        let sent: Vec<_> = messages
            .iter()
            .filter_map(|m| match m {
                AgentMessage::UserText { text, from } => Some((text.as_str(), *from)),
                _ => None,
            })
            .collect();
        let note = "The files were set back to how they were before your turn 1; \
                    changes from turn 1 are undone.";
        assert_eq!(
            sent,
            [
                ("go", MessageFrom::Person),
                (note, MessageFrom::Ovra),
                ("again", MessageFrom::Person)
            ]
        );
        // The next turn started: the go-back stays, its undo is gone.
        assert_eq!(fields(info().await), (Some(1), false));
        let undo = json!({"method": "agents.undo_go_back", "params": {"session": "s1"}});
        let refused = call(&f.core, undo).await;
        assert_eq!(refused.err().map(|e| e.code), Some(ErrorCode::Conflict));
    }
}
