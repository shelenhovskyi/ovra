//! `GhForge` against a fake `gh` script that records its arguments, stdin
//! and environment, and against a local bare repository as the remote.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use ovra_core::protocol::ErrorCode;
use ovra_core::protocol::pr::PrDraft;
use ovra_core::services::Forge;

use crate::GhForge;
use crate::run::Programs;

fn ok<T, E: std::fmt::Debug>(result: Result<T, E>) -> T {
    result.unwrap_or_else(|e| panic!("{e:?}"))
}

/// Runs the test's own git (not through the forge) and returns stdout.
fn git(dir: &Path, args: &[&str]) -> String {
    let mut command = Command::new("git");
    for (role, value) in [("NAME", "P"), ("EMAIL", "p@example.com")] {
        command.env(format!("GIT_AUTHOR_{role}"), value);
        command.env(format!("GIT_COMMITTER_{role}"), value);
    }
    let out = ok(command.args(args).current_dir(dir).output());
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// A project checkout `work` whose `origin` is the bare repository
/// `remote.git`, with `main` pushed to it.
struct Repo {
    _dir: tempfile::TempDir,
    work: PathBuf,
    remote: PathBuf,
}

impl Repo {
    fn new() -> Self {
        let dir = ok(tempfile::tempdir());
        let (work, remote) = (dir.path().join("work"), dir.path().join("remote.git"));
        git(
            dir.path(),
            &["init", "-q", "--bare", "-b", "main", "remote.git"],
        );
        git(dir.path(), &["init", "-q", "-b", "main", "work"]);
        git(
            &work,
            &["remote", "add", "origin", ok(remote.to_str().ok_or("path"))],
        );
        let repo = Self {
            _dir: dir,
            work,
            remote,
        };
        repo.commit("one");
        git(&repo.work, &["push", "-q", "origin", "main"]);
        repo
    }

    fn commit(&self, message: &str) -> String {
        git(
            &self.work,
            &["commit", "-q", "--allow-empty", "-m", message],
        );
        git(&self.work, &["rev-parse", "HEAD"])
    }

    fn remote_branch(&self, branch: &str) -> String {
        let head = format!("refs/heads/{branch}");
        let out = Command::new("git")
            .args(["rev-parse", "--verify", "--quiet", &head])
            .current_dir(&self.remote)
            .output();
        String::from_utf8_lossy(&ok(out).stdout).trim().to_owned()
    }

    fn forge(&self) -> GhForge {
        GhForge::with(Programs {
            gh: None,
            git: Some("git".into()),
        })
    }
}

/// A fake `gh` in its own folder: it appends its arguments (one per line,
/// then `--`) to `argv`, writes its stdin to `stdin` and its environment to
/// `env`, prints `version` for `--version` and `out` otherwise, and exits
/// with the code in `code` (0 without one).
struct FakeGh {
    dir: tempfile::TempDir,
}

impl FakeGh {
    fn new(out: &str) -> Self {
        let dir = ok(tempfile::tempdir());
        let script = "#!/bin/sh\nd=$(dirname \"$0\")\n\
            printf '%s\\n' \"$@\" -- >> \"$d/argv\"\ncat > \"$d/stdin\"\nenv > \"$d/env\"\n\
            case \"$1\" in --version) cat \"$d/version\";; *) cat \"$d/out\";; esac\n\
            exit $(cat \"$d/code\" 2>/dev/null || echo 0)\n";
        let fake = Self { dir };
        fake.write("gh", script);
        ok(std::fs::set_permissions(
            fake.gh(),
            std::fs::Permissions::from_mode(0o755),
        ));
        fake.write("out", out);
        fake.write("version", "gh version 2.60.0 (2024-10-14)\n");
        fake
    }

    fn write(&self, name: &str, text: &str) {
        ok(std::fs::write(self.dir.path().join(name), text));
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.dir.path().join(name)).unwrap_or_default()
    }

    fn gh(&self) -> PathBuf {
        self.dir.path().join("gh")
    }

    /// Each call's arguments.
    fn calls(&self) -> Vec<Vec<String>> {
        let argv = self.read("argv");
        let mut calls = vec![vec![]];
        for line in argv.lines() {
            if line == "--" {
                calls.push(vec![]);
            } else if let Some(call) = calls.last_mut() {
                call.push(line.to_owned());
            }
        }
        calls.pop();
        assert!(!argv.contains("--show-token"));
        calls
    }

    fn forge(&self, git: bool) -> GhForge {
        let git = git.then(|| PathBuf::from("git"));
        GhForge::with(Programs {
            gh: Some(self.gh()),
            git,
        })
    }
}

fn draft(draft: bool) -> PrDraft {
    PrDraft {
        remote: "origin".into(),
        base: "main".into(),
        branch: "ovra/t1/fix".into(),
        title: "--fix the login".into(),
        body: "Why:\n--draft is text here\n".into(),
        draft,
    }
}

#[tokio::test]
async fn fetch_returns_the_commit_the_remote_branch_is_at() {
    let repo = Repo::new();
    let first = repo.remote_branch("main");
    let fetched = repo
        .forge()
        .fetch(repo.work.clone(), "origin".into(), "main".into())
        .await;
    assert_eq!(ok(fetched), first);
    let missing = repo
        .forge()
        .fetch(repo.work.clone(), "origin".into(), "nope".into())
        .await;
    assert_eq!(missing.err().map(|e| e.code), Some(ErrorCode::NotFound));
}

#[tokio::test]
async fn fetch_and_push_leave_no_ref_behind_and_run_no_hooks() {
    let repo = Repo::new();
    let two = repo.commit("two");
    let hooks = repo.work.join(".git").join("hooks");
    ok(std::fs::create_dir_all(&hooks));
    let marker = repo.work.join("hook-ran");
    for hook in ["reference-transaction", "pre-push", "post-update"] {
        let path = hooks.join(hook);
        let script = format!("#!/bin/sh\necho {hook} >> '{}'\n", marker.display());
        ok(std::fs::write(&path, script));
        ok(std::fs::set_permissions(
            &path,
            std::fs::Permissions::from_mode(0o755),
        ));
    }
    // Someone's FETCH_HEAD names another commit: it is not read.
    ok(std::fs::write(
        repo.work.join(".git").join("FETCH_HEAD"),
        format!("{two}\t\tbranch 'x' of elsewhere\n"),
    ));
    let fetched = repo
        .forge()
        .fetch(repo.work.clone(), "origin".into(), "main".into())
        .await;
    assert_eq!(ok(fetched), repo.remote_branch("main"));
    assert_eq!(push(&repo, &two, None).await, None);
    let missing = repo
        .forge()
        .fetch(repo.work.clone(), "origin".into(), "nope".into())
        .await;
    assert_eq!(missing.err().map(|e| e.code), Some(ErrorCode::NotFound));
    assert_eq!(git(&repo.work, &["for-each-ref", "refs/ovra"]), "");
    assert!(!marker.exists(), "{}", ok(std::fs::read_to_string(&marker)));
}

/// Pushes `commit` to `ovra/t1/fix` on `origin`; the error code if refused.
async fn push(repo: &Repo, commit: &str, expect: Option<&str>) -> Option<ErrorCode> {
    let (commit, expect) = (commit.to_owned(), expect.map(str::to_owned));
    let branch = "ovra/t1/fix".to_owned();
    let forge = repo.forge();
    let pushed = forge.push(repo.work.clone(), "origin".into(), commit, branch, expect);
    pushed.await.err().map(|e| e.code)
}

#[tokio::test]
async fn a_push_creates_a_branch_and_then_only_moves_it_forward() {
    let repo = Repo::new();
    let one = repo.remote_branch("main");
    let two = repo.commit("two");
    assert_eq!(push(&repo, &two, None).await, None);
    assert_eq!(repo.remote_branch("ovra/t1/fix"), two);
    // Going back would overwrite: refused before anything is sent.
    assert_eq!(
        push(&repo, &one, Some(&two)).await,
        Some(ErrorCode::BadRequest)
    );
    let three = repo.commit("three");
    assert_eq!(push(&repo, &three, Some(&two)).await, None);
    assert_eq!(repo.remote_branch("ovra/t1/fix"), three);
}

#[tokio::test]
async fn a_push_is_refused_when_the_remote_branch_is_not_where_it_expects() {
    let repo = Repo::new();
    let ours = repo.commit("ours");
    let theirs = repo.commit("theirs");
    git(
        &repo.work,
        &[
            "push",
            "-q",
            "origin",
            &format!("{theirs}:refs/heads/ovra/t1/fix"),
        ],
    );
    let next = repo.commit("next");
    // It exists, but Ovra expects it not to; then it moved from `ours`.
    assert_eq!(push(&repo, &next, None).await, Some(ErrorCode::Conflict));
    assert_eq!(
        push(&repo, &next, Some(&ours)).await,
        Some(ErrorCode::Conflict)
    );
    assert_eq!(repo.remote_branch("ovra/t1/fix"), theirs);
}

#[tokio::test]
async fn remotes_and_branches_that_could_read_as_options_or_urls_are_refused() {
    let dir = ok(tempfile::tempdir());
    let forge = GhForge::with(Programs {
        gh: None,
        git: Some("git".into()),
    });
    let cases = [
        ("/srv/r.git", "main"),
        ("https://x:t@h.example/r", "main"),
        ("--upload-pack=x", "main"),
        ("origin", "-f"),
    ];
    for (remote, branch) in cases {
        let fetched = forge
            .fetch(dir.path().into(), remote.into(), branch.into())
            .await;
        assert_eq!(
            fetched.err().map(|e| e.code),
            Some(ErrorCode::BadRequest),
            "{remote} {branch}"
        );
    }
    let pushed = forge
        .push(
            dir.path().into(),
            "origin".into(),
            "HEAD".into(),
            "x".into(),
            None,
        )
        .await;
    assert_eq!(pushed.err().map(|e| e.code), Some(ErrorCode::BadRequest));
}

#[tokio::test]
async fn a_token_in_a_remote_url_never_reaches_an_error() {
    let repo = Repo::new();
    let url = "https://someone:ghp_SECRET123@github.invalid/o/r.git";
    git(&repo.work, &["remote", "add", "tok", url]);
    // No network: the proxy is a closed local port.
    git(&repo.work, &["config", "http.proxy", "http://127.0.0.1:1"]);
    let err = repo
        .forge()
        .fetch(repo.work.clone(), "tok".into(), "main".into())
        .await;
    let message = err.err().map(|e| e.message).unwrap_or_default();
    assert!(message.starts_with("git fetch failed"), "{message}");
    assert!(
        !message.contains("SECRET") && !message.contains("someone"),
        "{message}"
    );
}

#[tokio::test]
async fn status_asks_gh_about_the_remotes_host_without_the_token_flag() {
    let repo = Repo::new();
    git(
        &repo.work,
        &[
            "remote",
            "add",
            "gh",
            "https://x:ghp_SECRET@GitHub.com/o/r.git",
        ],
    );
    let fake = FakeGh::new(
        r#"{"hosts":{"github.com":[{"state":"success","active":true,"login":"octo"}]}}"#,
    );
    let status = ok(fake
        .forge(true)
        .status(repo.work.clone(), "gh".into())
        .await);
    insta::assert_json_snapshot!(status, @r#"
    {
      "installed": true,
      "version": "2.60.0",
      "host": "github.com",
      "login": "octo",
      "signed_in": true
    }
    "#);
    assert_eq!(
        fake.calls(),
        [
            vec!["--version"],
            vec![
                "auth",
                "status",
                "--active",
                "--hostname",
                "github.com",
                "--json",
                "hosts"
            ]
        ]
    );
    let env = fake.read("env");
    for var in [
        "GH_PROMPT_DISABLED=1",
        "GH_NO_UPDATE_NOTIFIER=1",
        "NO_COLOR=1",
        "GIT_TERMINAL_PROMPT=0",
    ] {
        assert!(env.lines().any(|line| line == var), "{var}");
    }

    // A local remote names no host: gh is not asked about one.
    let local = ok(fake
        .forge(true)
        .status(repo.work.clone(), "origin".into())
        .await);
    assert_eq!(
        (local.installed, local.host, local.signed_in),
        (true, None, false)
    );
    let missing = GhForge::with(Programs {
        gh: None,
        git: Some("git".into()),
    });
    let missing = ok(missing.status(repo.work.clone(), "gh".into()).await);
    assert_eq!(
        (missing.installed, missing.host.as_deref()),
        (false, Some("github.com"))
    );
}

#[tokio::test]
async fn create_passes_the_title_as_one_argument_and_the_body_on_stdin() {
    let repo = Repo::new();
    let dir = repo.work.clone();
    // A local remote names no repository: gh is not asked.
    let fake = FakeGh::new("https://github.com/o/r/pull/41\n");
    let local = fake.forge(true).create(dir.clone(), draft(true)).await;
    assert_eq!(local.err().map(|e| e.code), Some(ErrorCode::BadRequest));
    assert!(fake.calls().is_empty());

    let url = "https://x:ghp_SECRET@github.com/o/r.git";
    git(&dir, &["remote", "set-url", "origin", url]);
    git(
        &dir,
        &["remote", "add", "upstream", "https://github.com/up/r.git"],
    );
    let fake = FakeGh::new("https://github.com/o/r/pull/42\n");
    let made = ok(fake.forge(true).create(dir.clone(), draft(true)).await);
    assert_eq!(
        (made.number, made.url.as_str(), made.repo.as_str()),
        (42, "https://github.com/o/r/pull/42", "github.com/o/r")
    );
    let args = [
        "pr",
        "create",
        "--repo",
        "github.com/o/r",
        "--base",
        "main",
        "--head",
        "ovra/t1/fix",
    ];
    let mut want: Vec<&str> = args.to_vec();
    want.extend(["--title=--fix the login", "--body-file", "-", "--draft"]);
    assert_eq!(fake.calls(), [want.clone()]);
    assert_eq!(fake.read("stdin"), "Why:\n--draft is text here\n");

    let fake = FakeGh::new("https://github.com/o/r/pull/43\n");
    ok(fake.forge(true).create(dir, draft(false)).await);
    want.pop();
    assert_eq!(fake.calls(), [want]);
}

#[tokio::test]
async fn find_asks_for_open_pull_requests_from_the_branch_not_from_forks() {
    let repo = Repo::new();
    let dir = repo.work.clone();
    git(
        &dir,
        &["remote", "set-url", "origin", "https://github.com/o/r.git"],
    );
    let out = r#"[{"number":5,"url":"https://github.com/f/r/pull/5","isCrossRepository":true},
        {"number":6,"url":"https://github.com/o/r/pull/6","isCrossRepository":false}]"#;
    let fake = FakeGh::new(out);
    let (remote, branch) = ("origin".to_owned(), "ovra/t1/fix".to_owned());
    let forge = fake.forge(true);
    let found = forge.find(dir.clone(), remote.clone(), branch.clone());
    let found = ok(found.await).map(|i| (i.number, i.url, i.repo));
    let want = (
        "https://github.com/o/r/pull/6".to_owned(),
        "github.com/o/r".to_owned(),
    );
    assert_eq!(found, Some((6, want.0, want.1)));
    let args = [
        "pr",
        "list",
        "--repo",
        "github.com/o/r",
        "--head",
        "ovra/t1/fix",
        "--state",
        "open",
        "--json",
        "number,url,isCrossRepository",
    ];
    assert_eq!(fake.calls(), [args.to_vec()]);

    let fake = FakeGh::new("[]");
    let none = fake.forge(true).find(dir.clone(), remote, branch).await;
    assert_eq!(ok(none), None);
    let fake = FakeGh::new("[]");
    let odd = fake
        .forge(true)
        .find(dir, "origin".into(), "-x".into())
        .await;
    assert_eq!(odd.err().map(|e| e.code), Some(ErrorCode::BadRequest));
    assert!(fake.calls().is_empty());
}

#[tokio::test]
async fn view_and_failed_log_ask_for_exactly_what_they_read() {
    let dir = ok(tempfile::tempdir());
    let fake = FakeGh::new(
        r#"{"state":"MERGED","isDraft":false,"url":"https://github.com/o/r/pull/7","headRefOid":"abc"}"#,
    );
    let repo = || "github.com/o/r".to_owned();
    let view = ok(fake.forge(false).view(dir.path().into(), repo(), 7).await);
    assert_eq!((view.head.as_str(), view.checks.len()), ("abc", 0));
    let fields = "state,isDraft,url,headRefOid,statusCheckRollup,reviewDecision,mergeStateStatus";
    assert_eq!(
        fake.calls(),
        [vec![
            "pr",
            "view",
            "7",
            "--repo",
            "github.com/o/r",
            "--json",
            fields
        ]]
    );
    for bad in ["--repo=x/o/r", "o/r", ""] {
        let refused = fake
            .forge(false)
            .view(dir.path().into(), bad.into(), 7)
            .await;
        assert_eq!(refused.err().map(|e| e.code), Some(ErrorCode::BadRequest));
    }
    assert_eq!(fake.calls().len(), 1);

    let log: String = (0..250)
        .map(|i| format!("step {i} https://u:tok@h.example/x\n"))
        .collect();
    let fake = FakeGh::new(&log);
    let tail = ok(fake
        .forge(false)
        .failed_log(dir.path().into(), repo(), 9)
        .await);
    assert_eq!(tail.lines().count(), 200);
    assert!(
        tail.starts_with("step 50 ") && !tail.contains("tok@"),
        "{tail}"
    );
    assert_eq!(
        fake.calls(),
        [vec![
            "run",
            "view",
            "9",
            "--repo",
            "github.com/o/r",
            "--log-failed"
        ]]
    );

    // The 32 KiB cut falls inside the URL's user part.
    let wide = format!(
        "https://user:secret@h.example {}",
        "z".repeat(32 * 1024 - 15)
    );
    fake.write("out", &wide);
    let tail = ok(fake
        .forge(false)
        .failed_log(dir.path().into(), repo(), 9)
        .await);
    assert!(!tail.contains("cret") && tail.ends_with('z'), "{tail:.40}");

    fake.write("code", "1");
    fake.write("out", "");
    let failed = fake
        .forge(false)
        .failed_log(dir.path().into(), repo(), 9)
        .await;
    assert_eq!(failed.err().map(|e| e.code), Some(ErrorCode::Internal));
    let none = GhForge::with(Programs::default())
        .view(dir.path().into(), repo(), 7)
        .await;
    assert_eq!(none.err().map(|e| e.code), Some(ErrorCode::Unavailable));
}

#[tokio::test(start_paused = true)]
async fn a_gh_that_hangs_is_stopped_after_its_timeout() {
    let dir = ok(tempfile::tempdir());
    let fake = FakeGh::new("");
    fake.write("gh", "#!/bin/sh\nexec sleep 600\n");
    let started = tokio::time::Instant::now();
    let forge = fake.forge(false);
    let hung = forge
        .view(dir.path().into(), "github.com/o/r".into(), 1)
        .await;
    let err = hung.err().unwrap_or_else(|| panic!("no timeout"));
    assert_eq!(err.code, ErrorCode::Unavailable);
    assert_eq!(err.message, "gh pr view did not finish within 30 s");
    assert_eq!(started.elapsed(), crate::run::GH_TIMEOUT);
}

#[tokio::test]
async fn issues_are_only_read_from_the_remotes_repository() {
    let repo = Repo::new();
    let dir = repo.work.clone();
    git(
        &dir,
        &[
            "remote",
            "set-url",
            "origin",
            "https://tok@github.com/o/r.git",
        ],
    );
    let out = r#"[{"number":12,"title":"Login fails","url":"https://github.com/o/r/issues/12"}]"#;
    let fake = FakeGh::new(out);
    let forge = fake.forge(true);
    let found = ok(forge.issues(dir.clone(), "origin".into()).await);
    assert_eq!(
        found
            .iter()
            .map(|i| (i.number, i.title.as_str()))
            .collect::<Vec<_>>(),
        [(12, "Login fails")]
    );
    let list = [
        "issue",
        "list",
        "--repo",
        "github.com/o/r",
        "--state",
        "open",
        "--limit",
        "30",
        "--json",
        "number,title,url",
    ];
    assert_eq!(fake.calls(), [list.to_vec()]);
    let refused = forge.issues(dir.clone(), "--upload-pack=x".into()).await;
    assert_eq!(refused.err().map(|e| e.code), Some(ErrorCode::BadRequest));
    assert_eq!(fake.calls().len(), 1);

    let comments: Vec<String> = (0..12)
        .map(|i| {
            format!(r#"{{"author":{{"login":"u{i}"}},"body":"note {i} https://a:b@h.example/x"}}"#)
        })
        .collect();
    let view = format!(
        r#"{{"number":12,"title":"Login fails","url":"https://github.com/o/r/issues/12",
        "body":"Steps:\n1. Log in","comments":[{}]}}"#,
        comments.join(",")
    );
    let fake = FakeGh::new(&view);
    let issue = ok(fake
        .forge(true)
        .issue(dir.clone(), "origin".into(), 12)
        .await);
    // The latest ten comments, URLs without their user part.
    assert!(
        issue
            .text
            .starts_with("Steps:\n1. Log in\n\nComment by @u2:\nnote 2 https://h.example/x")
    );
    assert!(
        issue
            .text
            .ends_with("Comment by @u11:\nnote 11 https://h.example/x")
    );
    assert_eq!(
        fake.calls(),
        [vec![
            "issue",
            "view",
            "12",
            "--repo",
            "github.com/o/r",
            "--json",
            "number,title,url,body,comments"
        ]]
    );

    // Long comments go first, then the body's end.
    let long = format!(
        r#"{{"number":3,"title":"t","url":"u","body":"{}","comments":[{{"author":{{"login":"a"}},"body":"{}"}}]}}"#,
        "b".repeat(20 * 1024),
        "c".repeat(100)
    );
    let fake = FakeGh::new(&long);
    let issue = ok(fake.forge(true).issue(dir, "origin".into(), 3).await);
    assert!(issue.text.len() == 15 * 1024 && !issue.text.contains('c'));
}
