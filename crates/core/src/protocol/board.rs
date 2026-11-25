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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardEntry {
    pub id: EntryId,
    pub author: Author,
    pub body: EntryBody,
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
            },
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
          }
        }
        "#);
    }
}
