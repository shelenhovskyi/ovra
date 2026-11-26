//! `board.*`: a task's board, where its agents, Ovra and the person leave
//! claims, questions, answers and notes (`docs/specs/task-board.md`).

use serde::{Deserialize, Serialize};

use super::orchestration::TaskId;
use super::projects::ProjectId;

/// `b1`, `b2`, ... in the order entries were written, per task.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EntryId(pub String);

/// Who wrote an entry. The core sets it from the caller, never from text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Author {
    Person,
    Coordinator,
    Worker { name: String },
    Ovra,
}

/// Whom a question is for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Recipient {
    Person,
    Coordinator,
    Worker { name: String },
}

/// What an entry says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EntryBody {
    /// A worker takes these paths (relative, globs allowed).
    Claim {
        /// The worker the claim is for: its author, or the coordinator's
        /// worker.
        worker: String,
        paths: Vec<String>,
        note: String,
    },
    /// The worker gives these paths back.
    Release {
        worker: String,
        paths: Vec<String>,
    },
    Question {
        to: Recipient,
        text: String,
        /// Answers to pick from, so the person can answer with one tap.
        #[serde(flatten)]
        choices: Choices,
    },
    Answer {
        question: EntryId,
        text: String,
    },
    Note {
        text: String,
    },
    /// Something Ovra saw: an overlap, a review asked for, a stop.
    Ovra {
        text: String,
    },
}

/// A question's answers to pick from (2 to 4, each one short line), the
/// one the asker recommends and why. All empty for a free-text question.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Choices {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
    /// An index into `options`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardEntry {
    pub id: EntryId,
    pub author: Author,
    pub body: EntryBody,
    /// When it was written, Unix time in milliseconds; missing on entries
    /// saved before entries had a time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum BoardRequest {
    /// The task's entries from index `since`, oldest first.
    #[serde(rename = "board.list")]
    List {
        task: TaskId,
        #[serde(default)]
        since: usize,
    },
    /// The person's note.
    #[serde(rename = "board.post")]
    Post { task: TaskId, text: String },
    /// The person's answer to a question.
    #[serde(rename = "board.answer")]
    Answer {
        task: TaskId,
        question: EntryId,
        text: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum BoardResponse {
    #[serde(rename = "board.entries")]
    Entries {
        entries: Vec<BoardEntry>,
        /// The index after the last entry: pass it as `since` next time.
        next: usize,
        /// Questions for the person that nobody answered yet.
        open_for_person: Vec<EntryId>,
    },
    #[serde(rename = "board.entry")]
    Entry(BoardEntry),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum BoardEvent {
    /// A new entry on a task's board.
    #[serde(rename = "board.changed")]
    Changed {
        project: ProjectId,
        task: TaskId,
        entry: BoardEntry,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_read_as_the_ui_expects() {
        let entry = BoardEntry {
            id: EntryId("b2".into()),
            author: Author::Worker {
                name: "lint".into(),
            },
            body: EntryBody::Question {
                to: Recipient::Person,
                text: "Keep the old flag?".into(),
                choices: Choices::default(),
            },
            at_ms: Some(1_700_000_000_000),
        };
        insta::assert_json_snapshot!(entry, @r#"
        {
          "id": "b2",
          "author": {
            "kind": "worker",
            "name": "lint"
          },
          "body": {
            "kind": "question",
            "to": {
              "kind": "person"
            },
            "text": "Keep the old flag?"
          },
          "at_ms": 1700000000000
        }
        "#);
        let old: Result<BoardEntry, _> = serde_json::from_str(
            r#"{"id":"b1","author":{"kind":"ovra"},"body":{"kind":"note","text":"x"}}"#,
        );
        assert!(matches!(old, Ok(BoardEntry { at_ms: None, .. })));
    }

    #[test]
    fn a_question_with_options_reads_as_the_ui_expects_and_old_ones_still_load() {
        let body = EntryBody::Question {
            to: Recipient::Person,
            text: "Keep the old flag?".into(),
            choices: Choices {
                options: vec!["Keep".into(), "Drop".into()],
                recommended: Some(1),
                reason: Some("Nobody uses it".into()),
            },
        };
        insta::assert_json_snapshot!(body, @r#"
        {
          "kind": "question",
          "to": {
            "kind": "person"
          },
          "text": "Keep the old flag?",
          "options": [
            "Keep",
            "Drop"
          ],
          "recommended": 1,
          "reason": "Nobody uses it"
        }
        "#);
        let old: Result<EntryBody, _> =
            serde_json::from_str(r#"{"kind":"question","to":{"kind":"person"},"text":"?"}"#);
        assert!(
            matches!(old, Ok(EntryBody::Question { choices, .. }) if choices == Choices::default())
        );
    }
}
