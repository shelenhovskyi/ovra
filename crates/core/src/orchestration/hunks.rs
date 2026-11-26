//! Parts of a review item: `review.discard_hunk` throws one hunk of an
//! agent's change away before the person keeps the rest, and
//! `review.undo_discard` puts it back. Line comments for `review.comment`
//! are written into the message here too.

use std::collections::BTreeSet;
use std::future::Future;

use super::review::{self, Entry};
use super::{Result, conflict, not_found};
use crate::Core;
use crate::path::RelPath;
use crate::protocol::agents::SessionMode;
use crate::protocol::git::Hunk;
use crate::protocol::review::{ChangeKind, ChangedBy, LineComment, ReviewId, ReviewItem};
use crate::protocol::{ErrorCode, ProtocolError};

/// The most line comments one `review.comment` carries.
const MAX_LINE_COMMENTS: usize = 200;

/// What the last `review.discard_hunk` of an item did, while it can be
/// undone.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct Discarded {
    /// Snapshots of the workspace from just before and just after.
    before: String,
    after: String,
    path: String,
}

impl Discarded {
    /// The snapshots the undo needs, which pruning must keep.
    pub(super) fn snapshots(&self) -> [String; 2] {
        [self.before.clone(), self.after.clone()]
    }
}

/// Outcome of a discard or its undo: the item, conflicts, files written.
type Done = (ReviewItem, Vec<String>, Vec<String>);

/// `review.discard_hunk`.
pub(super) async fn discard(core: &Core, id: &ReviewId, path: &str, hunk: Hunk) -> Result<Done> {
    hunk.check()?;
    let path = RelPath::parse(path)?;
    let entry = review::find(core, id)?;
    workable(core, &entry, "discard parts")?;
    let files = review::files(core, &entry).await?;
    let file = files
        .iter()
        .find(|f| f.path == path.as_str())
        .ok_or_else(|| not_found(format!("a change to {path} in this review")))?;
    if file.kind == ChangeKind::Renamed {
        return Err(conflict(
            "a renamed file is kept or discarded with the whole review",
        ));
    }
    // Like reject: in the main checkout only the agent's own files, never
    // lines the person may have written.
    if entry.mode == SessionMode::Direct && file.by != ChangedBy::Agent {
        return Err(conflict(format!(
            "{path} was changed outside the agent too, so its parts are not discarded here: edit it yourself"
        )));
    }
    // An added or deleted file is one hunk, and only as a whole.
    let whole = match file.kind {
        ChangeKind::Added => hunk.old_lines == 0 && hunk.new_lines == file.additions,
        ChangeKind::Deleted => hunk.new_lines == 0 && hunk.old_lines == file.deletions,
        _ => true,
    };
    if !whole {
        return Err(ProtocolError::new(
            ErrorCode::BadRequest,
            format!("{path} is added or deleted as a whole; its hunk is the whole file"),
        ));
    }
    let kind = file.kind;
    let ws = entry.workspace.clone();
    let worktrees = core.worktrees();
    let rel = path.clone();
    let write = async move {
        let before = worktrees.snapshot(ws.dir.clone()).await?;
        if kind == ChangeKind::Deleted {
            // Still deleted as the diff showed it, or nothing is written.
            let now = worktrees
                .diff(ws.dir.clone(), ws.base.clone(), before.clone())
                .await?;
            let gone = now
                .iter()
                .any(|d| d.path == rel.as_str() && d.kind == ChangeKind::Deleted);
            if !gone {
                return Err(conflict(format!(
                    "{rel} is no longer deleted; look at its diff again"
                )));
            }
        } else {
            // Only what the person saw goes: the file must still hold the
            // hunk. An added file is then empty, and goes as in the base.
            worktrees
                .revert_hunk(ws.dir.clone(), rel.clone(), hunk)
                .await?;
        }
        if kind != ChangeKind::Modified {
            worktrees
                .restore(ws.dir.clone(), ws.base.clone(), vec![rel.clone()])
                .await?;
        }
        let after = worktrees.snapshot(ws.dir.clone()).await?;
        Ok(Discarded {
            before,
            after,
            path: rel.to_string(),
        })
    };
    let discarded = edit(core, &entry, path.as_str(), write).await?;
    let entry = review::update(core, id, |e| {
        e.discards.insert(discarded.path.clone());
        e.discarded = Some(discarded);
    })?;
    let item = review::item(core, &entry).await?;
    Ok((item, Vec::new(), vec![path.to_string()]))
}

/// `review.undo_discard`: the file as it was just before the last discard,
/// if nobody changed it since.
pub(super) async fn undo(core: &Core, id: &ReviewId) -> Result<Done> {
    let entry = review::find(core, id)?;
    let Some(discarded) = entry.discarded.clone() else {
        return Err(conflict("there is no discard to undo"));
    };
    workable(core, &entry, "undo the discard")?;
    let path = RelPath::parse(&discarded.path)?;
    let ws = entry.workspace.clone();
    let worktrees = core.worktrees();
    let rel = path.clone();
    let write = async move {
        let since = worktrees
            .changes(ws.dir.clone(), discarded.after.clone())
            .await?;
        let touched = since
            .iter()
            .any(|d| d.path == rel.as_str() || d.from.as_deref() == Some(rel.as_str()));
        if touched {
            return Err(conflict(format!(
                "{rel} changed since the discard, so it stays as it is"
            )));
        }
        worktrees
            .restore(ws.dir.clone(), discarded.before.clone(), vec![rel])
            .await
    };
    edit(core, &entry, path.as_str(), write).await?;
    let entry = review::update(core, id, |e| e.discarded = None)?;
    let item = review::item(core, &entry).await?;
    Ok((item, Vec::new(), vec![path.to_string()]))
}

/// Refuses to change a decided item's workspace, or one the agent works in.
fn workable(core: &Core, entry: &Entry, what: &str) -> Result<()> {
    review::check_open(entry)?;
    if review::busy(core, &entry.session) {
        return Err(conflict(format!(
            "the agent is still working on these changes; {what} once it is idle or stopped"
        )));
    }
    Ok(())
}

/// Runs `write` on the entry's workspace: for a direct session, through
/// direct mode, so the file stays the agent's (or the person's) as it was.
async fn edit<T>(
    core: &Core,
    entry: &Entry,
    path: &str,
    write: impl Future<Output = Result<T>>,
) -> Result<T> {
    if entry.mode == SessionMode::Direct {
        core.sessions()
            .direct
            .person_edit(&entry.session, path, write)
            .await
    } else {
        write.await
    }
}

/// The person's `text` with their `lines` comments after it, as one text
/// of theirs for the agent.
pub(super) fn with_lines(text: &str, lines: &[LineComment]) -> Result<String> {
    if lines.len() > MAX_LINE_COMMENTS {
        return Err(ProtocolError::new(
            ErrorCode::BadRequest,
            format!("at most {MAX_LINE_COMMENTS} line comments go in one message"),
        ));
    }
    let mut out = text.trim().to_owned();
    if lines.is_empty() {
        return Ok(out);
    }
    let mut sorted: Vec<&LineComment> = lines.iter().collect();
    for c in &sorted {
        RelPath::parse(&c.path)?;
        // A file name is the agent's choice: it must not add lines that
        // read as the person's.
        if c.path.chars().any(char::is_control) {
            return Err(ProtocolError::new(
                ErrorCode::BadRequest,
                "a line comment's file name has control characters",
            ));
        }
        if c.line == 0 || c.text.trim().is_empty() {
            return Err(ProtocolError::new(
                ErrorCode::BadRequest,
                "a line comment needs a line from 1 and some text",
            ));
        }
    }
    sorted.sort_by(|a, b| (&a.path, a.line, a.old).cmp(&(&b.path, b.line, b.old)));
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str("Comments on lines:");
    for c in sorted {
        let at = if c.old {
            format!(
                "{}, line {} of the old version (you removed it)",
                c.path, c.line
            )
        } else {
            format!("{}:{}", c.path, c.line)
        };
        let body = c.text.trim().replace('\n', "\n  ");
        out.push_str(&format!("\n- {at}: {body}"));
    }
    Ok(out)
}

/// Ovra's note to the agent that the person discarded parts of its
/// changes in `paths`, so its picture of them is out of date.
pub(super) fn discards_note(paths: &BTreeSet<String>) -> String {
    let list: Vec<&str> = paths.iter().map(String::as_str).collect();
    format!(
        "Ovra: the person discarded parts of your changes in these files, so they differ from what you last wrote; read them again before you change them:\n{}",
        crate::untrusted::untrusted("file names", &list.join("\n"))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lined(text: &str, lines: &[LineComment]) -> String {
        with_lines(text, lines).unwrap_or_else(|e| panic!("{e}"))
    }

    fn comment(path: &str, line: u32, old: bool, text: &str) -> LineComment {
        LineComment {
            path: path.into(),
            line,
            old,
            text: text.into(),
        }
    }

    #[test]
    fn line_comments_follow_the_text_in_file_and_line_order() {
        let lines = [
            comment("src/b.rs", 3, false, "Name this better."),
            comment(
                "src/a.rs",
                40,
                true,
                "Keep this check.\nIt guards the cache.",
            ),
            comment("src/a.rs", 12, false, " Why a clone? "),
        ];
        insta::assert_snapshot!(lined("Close, a few things:", &lines), @r"
        Close, a few things:

        Comments on lines:
        - src/a.rs:12: Why a clone?
        - src/a.rs, line 40 of the old version (you removed it): Keep this check.
          It guards the cache.
        - src/b.rs:3: Name this better.
        ");
        // Line comments alone are a message too.
        assert_eq!(
            lined("  ", &lines[2..]),
            "Comments on lines:\n- src/a.rs:12: Why a clone?"
        );
        assert_eq!(lined(" Plain. ", &[]), "Plain.");
    }

    #[test]
    fn line_comments_need_a_path_a_line_and_text() {
        for bad in [
            comment("../x.rs", 1, false, "no"),
            comment("x.rs", 0, false, "no"),
            comment("x.rs", 1, false, "  "),
        ] {
            assert!(with_lines("", &[bad]).is_err());
        }
        let many = vec![comment("x.rs", 1, false, "a"); MAX_LINE_COMMENTS + 1];
        assert!(with_lines("", &many).is_err());
    }
}
