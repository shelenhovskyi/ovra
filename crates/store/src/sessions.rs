//! Agent sessions and their transcripts in the project database.
//!
//! Contract: `ovra_core::services::SessionStore`; the in-memory reference
//! behaviour is `ovra_core::fakes::MemorySessions`. A project's sessions go
//! with it when it is removed.

use ovra_core::protocol::agents::{AgentMessage, SessionId, SessionInfo};
use ovra_core::services::{Result, SessionStore, StoredSession};
use rusqlite::{OptionalExtension, params};

/// The `settings` key of the orchestration state. The core drops what
/// belongs to removed projects and forgotten sessions when it loads it.
const ORCHESTRATION: &str = "orchestration";
/// The `settings` key of direct mode's state; the core drops what belongs
/// to forgotten sessions when it loads it.
const DIRECT: &str = "direct";

use crate::{Store, db_error, internal};

impl SessionStore for Store {
    fn load(&self, max_messages: usize) -> Result<Vec<(StoredSession, Vec<AgentMessage>)>> {
        let db = self.db();
        let mut sessions = db
            .prepare_cached("SELECT id, info, agent_session FROM agent_sessions ORDER BY rowid")
            .map_err(db_error)?;
        let mut messages = db
            .prepare_cached(
                "SELECT message FROM agent_messages WHERE session_id = ?1
                 ORDER BY id DESC LIMIT ?2",
            )
            .map_err(db_error)?;
        let rows = sessions
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get(2)?,
                ))
            })
            .map_err(db_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(db_error)?;
        let limit = i64::try_from(max_messages).unwrap_or(i64::MAX);
        let mut loaded = Vec::with_capacity(rows.len());
        for (id, info, agent_session) in rows {
            let info: SessionInfo = serde_json::from_str(&info)
                .map_err(|e| internal(format!("saved session {id}: {e}")))?;
            let mut transcript = messages
                .query_map(params![id, limit], |row| row.get::<_, String>(0))
                .map_err(db_error)?
                .map(|json| {
                    let json = json.map_err(db_error)?;
                    serde_json::from_str(&json)
                        .map_err(|e| internal(format!("transcript of session {id}: {e}")))
                })
                .collect::<Result<Vec<AgentMessage>>>()?;
            transcript.reverse();
            loaded.push((
                StoredSession {
                    info,
                    agent_session,
                },
                transcript,
            ));
        }
        Ok(loaded)
    }

    fn save(&self, session: &StoredSession) -> Result<()> {
        let info = serde_json::to_string(&session.info).map_err(|e| internal(e.to_string()))?;
        self.db()
            .execute(
                "INSERT INTO agent_sessions (id, project_id, info, agent_session)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (id) DO UPDATE SET
                    info = excluded.info, agent_session = excluded.agent_session",
                params![
                    session.info.id.0,
                    session.info.project.0,
                    info,
                    session.agent_session
                ],
            )
            .map_err(db_error)?;
        Ok(())
    }

    fn append(&self, session: &SessionId, message: &AgentMessage) -> Result<()> {
        let json = serde_json::to_string(message).map_err(|e| internal(e.to_string()))?;
        self.db()
            .execute(
                "INSERT INTO agent_messages (session_id, message) VALUES (?1, ?2)",
                params![session.0, json],
            )
            .map_err(db_error)?;
        Ok(())
    }

    fn remove(&self, session: &SessionId) -> Result<()> {
        self.db()
            .execute("DELETE FROM agent_sessions WHERE id = ?1", [&session.0])
            .map_err(db_error)?;
        Ok(())
    }

    fn load_orchestration(&self) -> Result<Option<String>> {
        self.setting(ORCHESTRATION)
    }

    fn save_orchestration(&self, json: &str) -> Result<()> {
        self.set_setting(ORCHESTRATION, json)
    }

    fn load_direct(&self) -> Result<Option<String>> {
        self.setting(DIRECT)
    }

    fn save_direct(&self, json: &str) -> Result<()> {
        self.set_setting(DIRECT, json)
    }
}

impl Store {
    fn setting(&self, key: &str) -> Result<Option<String>> {
        self.db()
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()
            .map_err(db_error)
    }

    fn set_setting(&self, key: &str, json: &str) -> Result<()> {
        self.db()
            .execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                params![key, json],
            )
            .map_err(db_error)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use ovra_core::fakes::MemorySessions;
    use ovra_core::protocol::agents::{AgentKind, AgentStatus, SessionMode, TurnUsage};
    use ovra_core::protocol::projects::ProjectId;
    use ovra_core::services::ProjectStore;
    use tempfile::TempDir;

    use super::*;

    fn session(id: &str, project: &ProjectId) -> StoredSession {
        StoredSession {
            info: SessionInfo {
                id: SessionId(id.into()),
                project: project.clone(),
                agent: AgentKind::Claude,
                mode: SessionMode::Direct,
                status: AgentStatus::Idle,
                title: format!("task {id}"),
                usage: TurnUsage::default(),
                resumable: false,
                billing: None,
                sandbox: None,
                sandbox_reported: false,
                account: None,
                undone_from: None,
                can_undo_go_back: false,
                no_web: false,
                model: None,
                started_ms: Some(1),
                active_ms: Some(2),
            },
            agent_session: None,
        }
    }

    fn said(text: &str) -> AgentMessage {
        AgentMessage::AssistantText {
            text: text.into(),
            parent: None,
        }
    }

    /// The behaviour every `SessionStore` must have.
    fn contract(store: &dyn SessionStore, project: &ProjectId) {
        let (mut a, b) = (session("s1", project), session("s2", project));
        store.save(&a).expect("save a");
        store.save(&b).expect("save b");
        for text in ["one", "two", "three"] {
            store.append(&a.info.id, &said(text)).expect("append");
        }
        a.agent_session = Some("claude-1".into());
        a.info.status = AgentStatus::Stopped;
        // A research session started without the web keeps that.
        a.info.mode = SessionMode::Research;
        a.info.no_web = true;
        store.save(&a).expect("update a");

        let loaded = store.load(2).expect("load");
        assert_eq!(
            loaded,
            [(a.clone(), vec![said("two"), said("three")]), (b, vec![])]
        );
        store.remove(&SessionId("s2".into())).expect("remove");
        let ids: Vec<_> = store
            .load(10)
            .expect("load")
            .into_iter()
            .map(|(s, _)| s.info.id.0)
            .collect();
        assert_eq!(ids, ["s1"]);

        assert_eq!(store.load_orchestration().expect("load"), None);
        store.save_orchestration("{\"a\":1}").expect("save");
        store.save_orchestration("{\"a\":2}").expect("replace");
        let kept = store.load_orchestration().expect("load");
        assert_eq!(kept.as_deref(), Some("{\"a\":2}"));

        assert_eq!(store.load_direct().expect("load"), None);
        store.save_direct("{\"d\":1}").expect("save");
        store.save_direct("{\"d\":2}").expect("replace");
        let kept = store.load_direct().expect("load");
        assert_eq!(kept.as_deref(), Some("{\"d\":2}"));
        // The two documents are kept apart.
        let orchestration = store.load_orchestration().expect("load");
        assert_eq!(orchestration.as_deref(), Some("{\"a\":2}"));
    }

    #[test]
    fn the_fake_keeps_sessions() {
        contract(&MemorySessions::default(), &ProjectId("p1".into()));
    }

    #[test]
    fn the_database_keeps_sessions_across_opens() {
        let dir = TempDir::new().expect("temp dir");
        let folder = dir.path().join("work");
        std::fs::create_dir(&folder).expect("folder");
        let data = dir.path().join("data");
        let store = Store::open(&data).expect("open");
        let project = store.add(Path::new(&folder)).expect("add").id;
        contract(&store, &project);
        drop(store);

        let store = Store::open(&data).expect("reopen");
        assert_eq!(store.load(10).expect("load").len(), 1);
        assert!(store.load_orchestration().expect("load").is_some());
        assert!(store.load_direct().expect("load").is_some());
        // A project's sessions go with it.
        ProjectStore::remove(&store, &project).expect("remove project");
        assert!(store.load(10).expect("load").is_empty());
    }
}
