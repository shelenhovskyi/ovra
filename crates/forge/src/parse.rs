//! What Ovra passes to `gh` and `git` and reads back: names are checked
//! before they become arguments, and URLs lose their user part before they
//! are shown. No process is started here.

use ovra_core::protocol::pr::{
    CheckState, Issue, IssueSummary, PrCheck, PrInfo, PrState, PrStatus,
};
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::Result;
use serde_json::Value;

/// `gh run view --log-failed` keeps this many lines from the end...
const LOG_LINES: usize = 200;
/// ...and at most this many bytes of them.
const LOG_BYTES: usize = 32 * 1024;

fn bad(message: &str) -> ProtocolError {
    ProtocolError::new(ErrorCode::BadRequest, message)
}

fn garbled(what: &str) -> ProtocolError {
    ProtocolError::new(
        ErrorCode::Internal,
        format!("could not read what {what} said"),
    )
}

/// A remote's plain name, such as `origin`: never a URL, a path or an
/// option. The name is not echoed: a caller could have passed a URL.
pub(crate) fn remote(name: &str) -> Result<()> {
    let plain = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
    let starts = name.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_');
    if name.is_empty() || name.len() > 100 || !plain || !starts {
        return Err(bad("a remote must be given by its name, such as origin"));
    }
    Ok(())
}

/// A branch name git accepts after `refs/heads/` (`git check-ref-format`)
/// that cannot be read as an option or a refspec.
pub(crate) fn branch(name: &str) -> Result<()> {
    let odd = |c: char| c.is_control() || c.is_whitespace() || "~^:?*[\\".contains(c);
    let bad_part = |p: &str| p.is_empty() || p.starts_with('.') || p.ends_with(".lock");
    if name.is_empty()
        || name.len() > 200
        || name.starts_with('-')
        || name.ends_with('.')
        || name == "@"
        || name.contains("..")
        || name.contains("@{")
        || name.chars().any(odd)
        || name.split('/').any(bad_part)
    {
        return Err(bad("not a valid branch name"));
    }
    Ok(())
}

/// A full commit id (SHA-1 or SHA-256).
pub(crate) fn object_id(id: &str) -> Result<()> {
    if !matches!(id.len(), 40 | 64) || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(bad("not a full commit id"));
    }
    Ok(())
}

/// The host `gh` signs in to for a remote URL (`https://`, `ssh://` or
/// scp-like `git@host:owner/repo`); `None` for a local path or a URL
/// without a dotted host name.
pub(crate) fn host(url: &str) -> Option<String> {
    let url = url.trim();
    let authority = match url.split_once("://") {
        Some((scheme, rest)) => {
            if !matches!(
                scheme,
                "https" | "http" | "ssh" | "git" | "git+ssh" | "ssh+git"
            ) {
                return None;
            }
            rest.split(['/', '?', '#']).next()?
        }
        None => {
            let (head, _) = url.split_once(':')?;
            if head.contains(['/', '\\']) {
                return None;
            }
            head
        }
    };
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let host = host.split(':').next()?.to_ascii_lowercase();
    let plain = host
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    (plain && host.contains('.') && !host.starts_with(['-', '.'])).then_some(host)
}

/// The repository a remote URL names, `HOST/OWNER/REPO`, for `gh --repo`;
/// `None` when it names no host or no `OWNER/REPO` path. The user part
/// is never kept.
pub(crate) fn repo(url: &str) -> Option<String> {
    let url = url.trim();
    let host = host(url)?;
    let path = match url.split_once("://") {
        Some((_, rest)) => rest.split_once('/')?.1,
        None => url.split_once(':')?.1,
    };
    let path = path.split(['?', '#']).next()?.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let repo = format!("{host}/{path}");
    repo_name(&repo).is_ok().then_some(repo)
}

/// A repository as `gh --repo` takes it: `HOST/OWNER/REPO` of plain names,
/// none of which can read as an option.
pub(crate) fn repo_name(repo: &str) -> Result<()> {
    let plain = |part: &str| {
        !part.is_empty()
            && part.len() <= 100
            && !part.starts_with(['-', '.'])
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    };
    let parts: Vec<&str> = repo.split('/').collect();
    if parts.len() != 3 || !parts.iter().all(|p| plain(p)) || !parts[0].contains('.') {
        return Err(bad(
            "not a repository on a host, such as github.com/owner/repo",
        ));
    }
    Ok(())
}

/// `text` with the user part (`user:token@`) removed from every URL in it.
pub(crate) fn scrub(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("://") {
        let (head, tail) = rest.split_at(at + 3);
        out.push_str(head);
        let end = tail
            .find(|c: char| c.is_whitespace() || "/?#'\"<>`".contains(c))
            .unwrap_or(tail.len());
        let authority = &tail[..end];
        out.push_str(
            authority
                .rsplit_once('@')
                .map_or(authority, |(_, host)| host),
        );
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

/// The version in `gh --version`'s first line, `gh version 2.60.0 (...)`.
pub(crate) fn gh_version(stdout: &str) -> Option<String> {
    let line = stdout.lines().next()?;
    let version = line
        .strip_prefix("gh version ")?
        .split_whitespace()
        .next()?;
    Some(version.to_owned())
}

/// The login and whether it works, from `gh auth status --json hosts`.
/// Only the `login` and `state` fields are read.
pub(crate) fn gh_auth(stdout: &str, host: &str) -> (Option<String>, bool) {
    let Ok(json) = serde_json::from_str::<Value>(stdout) else {
        return (None, false);
    };
    let Some(account) = json["hosts"][host].as_array().and_then(|a| a.first()) else {
        return (None, false);
    };
    let login = account["login"].as_str().map(str::to_owned);
    (login, account["state"].as_str() == Some("success"))
}

/// The pull request `gh pr create` printed the URL of, in `repo`.
pub(crate) fn created(stdout: &str, repo: String) -> Result<PrInfo> {
    let url = stdout
        .split_whitespace()
        .rev()
        .find(|word| word.starts_with("https://") && word.contains("/pull/"))
        .ok_or_else(|| garbled("gh pr create"))?;
    let number = url.rsplit("/pull/").next().and_then(|n| n.parse().ok());
    let number = number.ok_or_else(|| garbled("gh pr create"))?;
    Ok(PrInfo {
        number,
        url: scrub(url),
        repo,
    })
}

/// `gh pr list --json number,url,isCrossRepository`: the first pull
/// request from the repository itself; one from a fork with a branch of
/// the same name is not Ovra's.
pub(crate) fn found(stdout: &str, repo: String) -> Result<Option<PrInfo>> {
    let json: Value = serde_json::from_str(stdout).map_err(|_| garbled("gh pr list"))?;
    let list = json.as_array().ok_or_else(|| garbled("gh pr list"))?;
    let own = list.iter().find(|pr| pr["isCrossRepository"] == false);
    let Some(pr) = own else {
        return Ok(None);
    };
    let number = pr["number"].as_u64().ok_or_else(|| garbled("gh pr list"))?;
    let url = pr["url"].as_str().ok_or_else(|| garbled("gh pr list"))?;
    Ok(Some(PrInfo {
        number,
        url: scrub(url),
        repo,
    }))
}

/// An issue's text, at most: one `@` context item holds 16 KiB.
const ISSUE_BYTES: usize = 15 * 1024;
/// The latest comments an issue's text keeps, at most.
const ISSUE_COMMENTS: usize = 10;

/// `gh issue list --json number,title,url`.
pub(crate) fn issues(stdout: &str) -> Result<Vec<IssueSummary>> {
    let json: Value = serde_json::from_str(stdout).map_err(|_| garbled("gh issue list"))?;
    let list = json.as_array().ok_or_else(|| garbled("gh issue list"))?;
    list.iter()
        .map(|item| {
            let number = item["number"]
                .as_u64()
                .ok_or_else(|| garbled("gh issue list"))?;
            Ok(IssueSummary {
                number,
                title: item["title"].as_str().unwrap_or_default().to_owned(),
                url: scrub(item["url"].as_str().unwrap_or_default()),
            })
        })
        .collect()
}

/// `gh issue view --json number,title,url,body,comments`: the body, then
/// the latest comments, each under its author's login, cut to
/// [`ISSUE_BYTES`] by dropping the oldest comments first, then the
/// body's end. URLs lose their user part.
pub(crate) fn issue(stdout: &str) -> Result<Issue> {
    let json: Value = serde_json::from_str(stdout).map_err(|_| garbled("gh issue view"))?;
    let number = json["number"]
        .as_u64()
        .ok_or_else(|| garbled("gh issue view"))?;
    let body = scrub(json["body"].as_str().unwrap_or_default().trim());
    let comments: Vec<String> = json["comments"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| {
            let author = c["author"]["login"].as_str().unwrap_or("someone");
            let text = scrub(c["body"].as_str().unwrap_or_default().trim());
            format!("Comment by @{author}:\n{text}")
        })
        .collect();
    let mut kept = &comments[comments.len().saturating_sub(ISSUE_COMMENTS)..];
    let joined = |kept: &[String]| {
        let mut text = body.clone();
        for comment in kept {
            text.push_str("\n\n");
            text.push_str(comment);
        }
        text
    };
    let mut text = joined(kept);
    while text.len() > ISSUE_BYTES && !kept.is_empty() {
        kept = &kept[1..];
        text = joined(kept);
    }
    let mut end = text.len().min(ISSUE_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    Ok(Issue {
        number,
        title: json["title"].as_str().unwrap_or_default().to_owned(),
        url: scrub(json["url"].as_str().unwrap_or_default()),
        text,
    })
}

/// `gh pr view --json state,isDraft,url,headRefOid,statusCheckRollup,
/// reviewDecision,mergeStateStatus`.
pub(crate) fn pr_status(stdout: &str) -> Result<PrStatus> {
    let json: Value = serde_json::from_str(stdout).map_err(|_| garbled("gh pr view"))?;
    let text = |key: &str| {
        json[key]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    let state = match json["state"].as_str() {
        Some("OPEN") => PrState::Open,
        Some("CLOSED") => PrState::Closed,
        Some("MERGED") => PrState::Merged,
        _ => return Err(garbled("gh pr view")),
    };
    let checks = json["statusCheckRollup"].as_array();
    Ok(PrStatus {
        state,
        draft: json["isDraft"].as_bool().unwrap_or(false),
        url: text("url").map(|u| scrub(&u)).unwrap_or_default(),
        head: text("headRefOid").unwrap_or_default(),
        checks: checks.into_iter().flatten().map(check).collect(),
        review_decision: text("reviewDecision"),
        merge_state: text("mergeStateStatus"),
    })
}

/// A `CheckRun` (GitHub Actions and other apps) or a commit status
/// (`StatusContext`) of the rollup.
fn check(item: &Value) -> PrCheck {
    let text = |key: &str| item[key].as_str().unwrap_or_default();
    let url = |key: &str| Some(text(key)).filter(|u| !u.is_empty()).map(scrub);
    if text("__typename") == "StatusContext" {
        let state = match text("state") {
            "SUCCESS" => CheckState::Passed,
            "PENDING" | "EXPECTED" => CheckState::Pending,
            _ => CheckState::Failed,
        };
        return PrCheck {
            name: text("context").to_owned(),
            state,
            url: url("targetUrl"),
            run: None,
        };
    }
    let state = match (text("status"), text("conclusion")) {
        ("COMPLETED", "SUCCESS") => CheckState::Passed,
        ("COMPLETED", "NEUTRAL" | "SKIPPED" | "STALE") => CheckState::Skipped,
        ("COMPLETED", _) => CheckState::Failed,
        _ => CheckState::Pending,
    };
    let url = url("detailsUrl");
    PrCheck {
        name: text("name").to_owned(),
        state,
        run: url.as_deref().and_then(run_id),
        url,
    }
}

/// The workflow run of a check page, `.../actions/runs/<run>/job/<job>`.
fn run_id(url: &str) -> Option<u64> {
    let rest = url.split_once("/actions/runs/")?.1;
    rest.split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}

/// The end of a failed log: its last 200 lines, at most 32 KiB of them.
pub(crate) fn log_tail(log: &str) -> &str {
    let log = log.trim_end_matches(['\n', '\r']);
    let start = log
        .rmatch_indices('\n')
        .nth(LOG_LINES - 1)
        .map_or(0, |(at, _)| at + 1);
    let tail = &log[start..];
    let mut cut = tail.len().saturating_sub(LOG_BYTES);
    while !tail.is_char_boundary(cut) {
        cut += 1;
    }
    &tail[cut..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_remote_names_pass() {
        for ok in ["origin", "up-stream", "my_fork.2"] {
            assert!(remote(ok).is_ok(), "{ok}");
        }
        for no in [
            "",
            "-u",
            "--upload-pack=x",
            ".hidden",
            "https://x:tok@github.com/o/r",
            "../r",
            "o r",
            "git@github.com:o/r",
        ] {
            assert!(remote(no).is_err(), "{no}");
        }
    }

    #[test]
    fn branch_names_follow_git_and_never_read_as_options() {
        for ok in ["main", "ovra/t1/fix-login", "release-1.2"] {
            assert!(branch(ok).is_ok(), "{ok}");
        }
        for no in [
            "", "-f", "--force", "a..b", "a b", "a:b", "x/", "/x", "a//b", ".x", "x.lock", "x.",
            "@", "a@{1}", "a~1", "a^", "a\\b", "a\nb",
        ] {
            assert!(branch(no).is_err(), "{no:?}");
        }
        assert!(object_id(&"a".repeat(40)).is_ok());
        assert!(object_id(&"a".repeat(64)).is_ok());
        assert!(object_id("HEAD").is_err());
        assert!(object_id(&format!("-{}", "a".repeat(39))).is_err());
    }

    #[test]
    fn the_host_comes_from_any_remote_url_form() {
        let cases = [
            ("https://github.com/o/r.git", Some("github.com")),
            ("https://user:ghp_secret@GitHub.com/o/r", Some("github.com")),
            (
                "ssh://git@ghe.example.com:2222/o/r.git",
                Some("ghe.example.com"),
            ),
            ("git@github.com:o/r.git", Some("github.com")),
            ("/srv/git/r.git", None),
            ("file:///srv/git/r.git", None),
            ("C:\\repos\\r", None),
            ("ext::sh -c x", None),
            ("https://-x.com/o/r", None),
        ];
        for (url, want) in cases {
            assert_eq!(host(url).as_deref(), want, "{url}");
        }
    }

    #[test]
    fn the_repository_comes_from_the_remote_url_without_its_user_part() {
        let cases = [
            ("https://github.com/o/r.git", Some("github.com/o/r")),
            ("https://u:ghp_x@GitHub.com/o/r/", Some("github.com/o/r")),
            (
                "ssh://git@ghe.example.com:2222/o/r.git",
                Some("ghe.example.com/o/r"),
            ),
            ("git@github.com:o/r.git", Some("github.com/o/r")),
            ("git@github.com:o/r.git\n", Some("github.com/o/r")),
            ("https://github.com/o", None),
            ("https://github.com/o/r/x", None),
            ("https://github.com/-o/r", None),
            ("https://github.com/o/r%20x", None),
            ("/srv/git/o/r.git", None),
        ];
        for (url, want) in cases {
            assert_eq!(repo(url).as_deref(), want, "{url}");
        }
        assert!(repo_name("github.com/o/r").is_ok());
        for no in ["", "o/r", "github.com/o/r/x", "--repo=x/o/r", "h.com/o/.r"] {
            assert!(repo_name(no).is_err(), "{no}");
        }
    }

    #[test]
    fn scrubbing_removes_the_user_part_of_every_url() {
        let text = "fatal: unable to access 'https://user:ghp_secret@github.com/o/r.git/': \
                    and http://tok@h.example/x, ssh://git@github.com:22/o and https://github.com/ok";
        let clean = scrub(text);
        assert!(
            !clean.contains("ghp_secret") && !clean.contains("tok@"),
            "{clean}"
        );
        assert_eq!(
            clean,
            "fatal: unable to access 'https://github.com/o/r.git/': \
             and http://h.example/x, ssh://github.com:22/o and https://github.com/ok"
        );
    }

    #[test]
    fn gh_answers_are_read_by_field() {
        let version =
            "gh version 2.60.0 (2024-10-14)\nhttps://github.com/cli/cli/releases/tag/v2.60.0\n";
        assert_eq!(gh_version(version).as_deref(), Some("2.60.0"));
        assert_eq!(gh_version("something else"), None);
        let auth = r#"{"hosts":{"github.com":[{"state":"success","active":true,
            "host":"github.com","login":"octo","tokenSource":"keyring"}]}}"#;
        assert_eq!(gh_auth(auth, "github.com"), (Some("octo".into()), true));
        assert_eq!(gh_auth(auth, "ghe.example.com"), (None, false));
        let failed = r#"{"hosts":{"github.com":[{"state":"error","login":"octo"}]}}"#;
        assert_eq!(gh_auth(failed, "github.com"), (Some("octo".into()), false));
        assert_eq!(gh_auth("not json", "github.com"), (None, false));
        let made = created(
            "Creating pull request\nhttps://github.com/o/r/pull/12\n",
            "github.com/o/r".into(),
        );
        assert_eq!(
            made.map(|p| (p.number, p.url)).ok(),
            Some((12, "https://github.com/o/r/pull/12".into()))
        );
        assert!(created("oops", String::new()).is_err());
    }

    #[test]
    fn a_pull_requests_view_maps_to_its_status_and_checks() {
        let view = r#"{"state":"OPEN","isDraft":true,"url":"https://github.com/o/r/pull/7",
          "headRefOid":"abc","reviewDecision":"","mergeStateStatus":"BLOCKED",
          "statusCheckRollup":[
            {"__typename":"CheckRun","name":"test","status":"COMPLETED","conclusion":"FAILURE",
             "detailsUrl":"https://github.com/o/r/actions/runs/123/job/456"},
            {"__typename":"CheckRun","name":"lint","status":"COMPLETED","conclusion":"SUCCESS",
             "detailsUrl":"https://github.com/o/r/actions/runs/124/job/457"},
            {"__typename":"CheckRun","name":"e2e","status":"IN_PROGRESS","conclusion":"",
             "detailsUrl":"https://github.com/o/r/actions/runs/125/job/458"},
            {"__typename":"CheckRun","name":"docs","status":"COMPLETED","conclusion":"SKIPPED",
             "detailsUrl":"https://github.com/o/r/actions/runs/126/job/459"},
            {"__typename":"CheckRun","name":"app","status":"COMPLETED","conclusion":"CANCELLED",
             "detailsUrl":"https://ci.example.com/build/9"},
            {"__typename":"StatusContext","context":"ci/legacy","state":"PENDING",
             "targetUrl":"https://u:tok@ci.example.com/7"}
          ]}"#;
        let status = pr_status(view).unwrap_or_else(|e| panic!("{e:?}"));
        insta::assert_json_snapshot!(status);
        assert!(pr_status(r#"{"state":"DRAFT"}"#).is_err());
        assert!(pr_status("").is_err());
    }

    #[test]
    fn the_log_keeps_its_last_200_lines_and_32_kib() {
        let lines: Vec<String> = (0..300).map(|i| format!("line {i}")).collect();
        let log = lines.join("\n") + "\n";
        let tail = log_tail(&log);
        assert_eq!(tail.lines().count(), 200);
        assert!(tail.starts_with("line 100\n") && tail.ends_with("line 299"));
        let wide = "\u{e9}".repeat(40_000);
        let tail = log_tail(&wide);
        assert!(tail.len() <= 32 * 1024 && tail.len() > 32 * 1024 - 2);
        assert_eq!(log_tail("short"), "short");
    }
}
