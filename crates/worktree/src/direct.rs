//! Direct mode (ADR 0005) end to end against real git: the core's
//! baseline, attribution and reject over [`GitWorktrees`] in a temporary
//! repository, with the person and the agent changing the same checkout.
//! The agent's tool calls are driven by hand through its `AgentHost`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use ovra_core::Core;
use ovra_core::fakes::MemoryProjects;
use ovra_core::protocol::agents::{AgentMessage, AgentStatus};
use ovra_core::protocol::review::{ChangedBy, ReviewItem, ReviewResponse, ReviewState};
use ovra_core::protocol::{Request, Response};
use ovra_core::services::{AgentHost, AgentRuntime, AgentSpec, Result, RunningAgent};
use serde_json::{Value, json};
use tempfile::TempDir;

use super::GitWorktrees;

/// A runtime whose sessions the test drives through their hosts.
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
        self.hosts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(host);
        Ok(Box::new(Quiet))
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new(crate::git::program())
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap_or_else(|e| panic!("git: {e}"));
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

struct Checkout {
    _tmp: TempDir,
    root: PathBuf,
    core: Core,
    host: AgentHost,
    tools: u32,
}

impl Checkout {
    /// A repository with `committed` files, then `uncommitted` ones written
    /// on top, and a direct session started in it.
    async fn start(committed: &[(&str, &[u8])], uncommitted: &[(&str, &[u8])]) -> Self {
        let tmp = tempfile::Builder::new()
            .prefix("ovra direct \u{e9} ")
            .tempdir()
            .unwrap_or_else(|e| panic!("{e}"));
        let root = tmp.path().join("main");
        fs::create_dir_all(&root).unwrap_or_else(|e| panic!("{e}"));
        git(&root, &["init", "--quiet", "--initial-branch=main"]);
        let config = root.join(".git").join("config");
        let mut text = fs::read_to_string(&config).unwrap_or_default();
        text.push_str(
            "[user]\n\tname = Test\n\temail = test@example.com\n\
             [commit]\n\tgpgsign = false\n\
             [core]\n\tautocrlf = false\n\thooksPath = no-hooks\n",
        );
        fs::write(&config, text).unwrap_or_else(|e| panic!("{e}"));
        for (path, bytes) in committed {
            put(&root, path, bytes);
        }
        git(&root, &["add", "--all"]);
        git(&root, &["commit", "--quiet", "-m", "start"]);
        for (path, bytes) in uncommitted {
            put(&root, path, bytes);
        }
        let runtime = Arc::new(Manual::default());
        let core = Core::builder("test")
            .projects(Arc::new(MemoryProjects::default()))
            .agents(runtime.clone())
            .worktrees(Arc::new(GitWorktrees::new()))
            .build();
        let root_text = root.to_string_lossy().into_owned();
        ok(
            &core,
            json!({"method": "projects.add", "params": {"root": root_text}}),
        )
        .await;
        ok(
            &core,
            json!({"method": "agents.start", "params": {
                "project": "p1", "agent": "claude", "mode": "direct", "prompt": "go"}}),
        )
        .await;
        let host = runtime
            .hosts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)[0]
            .clone();
        Self {
            _tmp: tmp,
            root,
            core,
            host,
            tools: 0,
        }
    }

    fn put(&self, path: &str, bytes: &[u8]) {
        put(&self.root, path, bytes);
    }

    fn read(&self, path: &str) -> Option<Vec<u8>> {
        fs::read(self.root.join(path)).ok()
    }

    /// One tool call of the agent: `change` runs while the tool does.
    async fn tool(&mut self, name: &str, input: Value, change: impl FnOnce(&Path)) {
        self.tools += 1;
        let id = format!("toolu_{}", self.tools);
        self.host
            .before_tool(&json!({"tool_use_id": id, "tool_name": name, "tool_input": input}))
            .await;
        change(&self.root);
        self.host.message(AgentMessage::ToolResult {
            tool_use_id: id,
            output: String::new(),
            is_error: false,
            parent: None,
        });
        self.host.tools_settled().await;
    }

    async fn write(&mut self, path: &str, bytes: &[u8]) {
        let input = json!({"file_path": self.root.join(path), "content": ""});
        self.tool("Write", input, |root| put(root, path, bytes))
            .await;
    }

    async fn bash(&mut self, command: &str, change: impl FnOnce(&Path)) {
        self.tool("Bash", json!({"command": command}), change).await;
    }

    async fn item(&self) -> ReviewItem {
        let list = ok(
            &self.core,
            json!({"method": "review.list", "params": {"project": "p1"}}),
        )
        .await;
        let Response::Review(ReviewResponse::List(mut items)) = list else {
            panic!("{list:?}");
        };
        items.pop().unwrap_or_else(|| panic!("no review item"))
    }

    /// Who changed what, as the review shows it.
    async fn by(&self) -> Vec<(String, Option<String>, ChangedBy)> {
        self.item()
            .await
            .files
            .into_iter()
            .map(|f| (f.path, f.from, f.by))
            .collect()
    }

    async fn decide(&self, method: &str, extra: Value) -> (ReviewItem, Vec<String>) {
        // The session's item: the only one, so the first.
        let mut params = json!({"id": "r1"});
        if let (Some(params), Some(extra)) = (params.as_object_mut(), extra.as_object()) {
            params.extend(extra.clone());
        }
        let done = ok(&self.core, json!({"method": method, "params": params})).await;
        let Response::Review(ReviewResponse::Done {
            item, conflicts, ..
        }) = done
        else {
            panic!("{done:?}");
        };
        (item, conflicts)
    }

    /// HEAD, the staged changes and the stash: what direct mode must not
    /// touch except through an approved commit.
    fn git_state(&self) -> (String, String, String) {
        (
            git(&self.root, &["rev-parse", "HEAD"]),
            git(&self.root, &["diff", "--cached", "--name-status"]),
            git(&self.root, &["stash", "list"]),
        )
    }
}

fn put(root: &Path, path: &str, bytes: &[u8]) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap_or(root)).unwrap_or_else(|e| panic!("{e}"));
    fs::write(path, bytes).unwrap_or_else(|e| panic!("{e}"));
}

async fn ok(core: &Core, json: Value) -> Response {
    let request: Request = serde_json::from_value(json).unwrap_or_else(|e| panic!("{e}"));
    core.handle(request)
        .await
        .unwrap_or_else(|e| panic!("{e:?}"))
}

/// Not valid UTF-8, with NUL bytes and a CR LF: git treats it as binary.
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\xff\xfe";
/// Longer than [`PNG`]: see the note on sizes below.
const PNG_EDITED: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x01\x00\x02";

fn row(path: &str, from: Option<&str>, by: ChangedBy) -> (String, Option<String>, ChangedBy) {
    (path.to_owned(), from.map(str::to_owned), by)
}

// Every version of a file in this test has a size of its own. It runs in
// well under a second, so its rewrites can share file times, and a rewrite
// of the same size and time then rests on git's racy-clean checks alone.
// Those are git's to test; here a size change always tells git to look.
#[tokio::test]
async fn reject_keeps_the_persons_work_and_touches_no_git_state() {
    use ChangedBy::{Agent, Both, Outside};
    let mut c = Checkout::start(
        &[
            ("shared.txt", b"one\ntwo\n"),
            ("plain.txt", b"plain\n"),
            ("logo.png", PNG),
            (".gitignore", b".env\n"),
        ],
        // The person's uncommitted work is part of the baseline.
        &[
            ("plain.txt", b"plain, edited before\n"),
            ("draft.md", b"draft\n"),
            (".env", b"SECRET=1\n"),
        ],
    )
    .await;
    let index = fs::read(c.root.join(".git").join("index")).unwrap_or_default();
    let before = c.git_state();

    // The person edits shared.txt first; the agent then edits it too.
    c.put("shared.txt", b"one\ntwo\nmine\n");
    c.write("shared.txt", b"one\ntwo\nmine\nagent\n").await;
    c.bash("make", |root| {
        put(root, "plain.txt", b"plain, rewritten by the agent\n");
        put(root, "logo.png", PNG_EDITED);
        put(root, "new.bin", PNG);
        // An ignored file is never in the diff, so never reverted.
        put(root, ".env", b"SECRET=agent\n");
    })
    .await;
    // Afterwards, in the editor.
    c.put("draft.md", b"draft, edited after\n");

    assert_eq!(
        c.by().await,
        [
            row("draft.md", None, Outside),
            row("logo.png", None, Agent),
            row("new.bin", None, Agent),
            row("plain.txt", None, Agent),
            row("shared.txt", None, Both),
        ]
    );
    c.host.status(AgentStatus::Stopped);
    let (item, conflicts) = c.decide("review.reject", json!({})).await;
    assert_eq!(item.state, ReviewState::Rejected);
    assert_eq!(conflicts, ["shared.txt"]);

    let text = |path| {
        c.read(path)
            .map(|b| String::from_utf8_lossy(&b).into_owned())
    };
    assert_eq!(
        text("shared.txt").as_deref(),
        Some("one\ntwo\nmine\nagent\n")
    );
    assert_eq!(text("plain.txt").as_deref(), Some("plain, edited before\n"));
    assert_eq!(
        c.read("logo.png").as_deref(),
        Some(PNG),
        "binary, byte for byte"
    );
    assert_eq!(c.read("new.bin"), None);
    assert_eq!(text("draft.md").as_deref(), Some("draft, edited after\n"));
    assert_eq!(text(".env").as_deref(), Some("SECRET=agent\n"));
    assert_eq!(c.git_state(), before, "HEAD, index and stash untouched");
    let after = fs::read(c.root.join(".git").join("index")).unwrap_or_default();
    assert_eq!(after, index, "the real index file is byte-identical");
}

#[tokio::test]
async fn reject_undoes_the_agents_renames_and_deletes() {
    use ChangedBy::{Agent, Outside};
    let mut c = Checkout::start(
        &[
            ("old name.txt", b"moved\nwith history\n"),
            ("gone.txt", b"gone\n"),
            ("kept.txt", b"kept\n"),
        ],
        &[],
    )
    .await;
    c.bash("git mv && rm", |root| {
        fs::rename(root.join("old name.txt"), root.join("new name.txt"))
            .unwrap_or_else(|e| panic!("{e}"));
        fs::remove_file(root.join("gone.txt")).unwrap_or_else(|e| panic!("{e}"));
    })
    .await;
    // The person deletes a file of their own meanwhile.
    fs::remove_file(c.root.join("kept.txt")).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        c.by().await,
        [
            row("gone.txt", None, Agent),
            row("kept.txt", None, Outside),
            row("new name.txt", Some("old name.txt"), Agent),
        ]
    );
    let (_, conflicts) = c.decide("review.reject", json!({})).await;
    assert!(conflicts.is_empty());
    assert_eq!(
        c.read("old name.txt").as_deref(),
        Some(&b"moved\nwith history\n"[..])
    );
    assert_eq!(c.read("new name.txt"), None);
    assert_eq!(c.read("gone.txt").as_deref(), Some(&b"gone\n"[..]));
    assert_eq!(c.read("kept.txt"), None, "the person's delete stays");

    // Undo puts the agent's rename and delete back, from the snapshot.
    let (item, conflicts) = c.decide("review.undo_reject", json!({})).await;
    assert_eq!(item.state, ReviewState::Open);
    assert!(conflicts.is_empty());
    assert_eq!(c.read("old name.txt"), None);
    assert_eq!(
        c.read("new name.txt").as_deref(),
        Some(&b"moved\nwith history\n"[..])
    );
    assert_eq!(c.read("gone.txt"), None);
}

#[tokio::test]
async fn approve_commits_only_the_agents_files() {
    let mut c = Checkout::start(&[("a.txt", b"a\n"), ("b.txt", b"b\n")], &[]).await;
    // The person has a change staged; it stays staged and out of the commit.
    c.put("b.txt", b"b, staged\n");
    git(&c.root, &["add", "b.txt"]);
    c.write("a.txt", b"agent\n").await;
    c.write("c.txt", b"agent\n").await;
    c.host.status(AgentStatus::Stopped);
    let (item, conflicts) = c
        .decide("review.approve", json!({"commit": "feat: agent work"}))
        .await;
    assert_eq!(item.state, ReviewState::Approved);
    assert!(conflicts.is_empty());
    let committed = git(&c.root, &["show", "--name-only", "--format=%s", "HEAD"]);
    assert_eq!(committed, "feat: agent work\n\na.txt\nc.txt");
    assert_eq!(
        git(&c.root, &["diff", "--cached", "--name-only"]),
        "b.txt",
        "the person's staged change is still staged"
    );
    assert_eq!(c.read("a.txt").as_deref(), Some(&b"agent\n"[..]));
}
