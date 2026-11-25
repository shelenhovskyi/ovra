//! The audit log in the project database.
//!
//! Contract: `ovra_core::services::AuditLog`; the in-memory reference
//! behaviour is `ovra_core::fakes::MemoryAudit`. Entries are only appended:
//! the table's triggers refuse edits and deletes.

use ovra_core::protocol::agents::SessionId;
use ovra_core::protocol::audit::{AuditEntry, AuditEvent, AuditOrder, AuditRange};
use ovra_core::protocol::projects::ProjectId;
use ovra_core::services::{AuditLog, Result};
use rusqlite::params;

use crate::{Store, db_error, internal, now_ms};

impl AuditLog for Store {
    fn append(
        &self,
        project: &ProjectId,
        session: Option<&SessionId>,
        event: AuditEvent,
    ) -> Result<AuditEntry> {
        let json = serde_json::to_string(&event)
            .map_err(|e| internal(format!("writing an audit entry: {e}")))?;
        let at_ms = now_ms();
        let seq: i64 = self
            .db()
            .query_row(
                "INSERT INTO audit (at_ms, project_id, session_id, event)
                 VALUES (?1, ?2, ?3, ?4) RETURNING seq",
                params![
                    i64::try_from(at_ms).unwrap_or(i64::MAX),
                    project.0,
                    session.map(|s| &s.0),
                    json
                ],
                |row| row.get(0),
            )
            .map_err(db_error)?;
        Ok(AuditEntry {
            seq: u64::try_from(seq).unwrap_or(0),
            at_ms,
            project: project.clone(),
            session: session.cloned(),
            event,
        })
    }

    fn list(
        &self,
        project: &ProjectId,
        range: AuditRange,
        limit: usize,
    ) -> Result<Vec<AuditEntry>> {
        let db = self.db();
        let mut stmt = db
            .prepare_cached(match range.order {
                AuditOrder::OldestFirst => {
                    "SELECT seq, at_ms, session_id, event FROM audit
                     WHERE project_id = ?1 AND seq > ?2 AND seq < ?3 ORDER BY seq LIMIT ?4"
                }
                AuditOrder::NewestFirst => {
                    "SELECT seq, at_ms, session_id, event FROM audit
                     WHERE project_id = ?1 AND seq > ?2 AND seq < ?3 ORDER BY seq DESC LIMIT ?4"
                }
            })
            .map_err(db_error)?;
        let seq = |s: u64| i64::try_from(s).unwrap_or(i64::MAX);
        let after = range.after.map_or(0, seq);
        let before = range.before.map_or(i64::MAX, seq);
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let rows = stmt
            .query_map(params![project.0, after, before, limit], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .map_err(db_error)?;
        rows.map(|row| {
            let (seq, at_ms, session, event) = row.map_err(db_error)?;
            let event = serde_json::from_str(&event)
                .map_err(|e| internal(format!("audit entry {seq}: {e}")))?;
            Ok(AuditEntry {
                seq: u64::try_from(seq).unwrap_or(0),
                at_ms: u64::try_from(at_ms).unwrap_or(0),
                project: project.clone(),
                session: session.map(SessionId),
                event,
            })
        })
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use ovra_core::fakes::MemoryAudit;
    use ovra_core::services::ProjectStore;
    use tempfile::TempDir;

    use super::*;

    fn project(id: &str) -> ProjectId {
        ProjectId(id.into())
    }

    fn trusted(trusted: bool) -> AuditEvent {
        AuditEvent::TrustChanged {
            trusted,
            fingerprint: Some("9f2c".into()),
        }
    }

    fn append(log: &dyn AuditLog, p: &str, s: Option<&str>, t: bool) -> AuditEntry {
        let session = s.map(|s| SessionId(s.into()));
        log.append(&project(p), session.as_ref(), trusted(t))
            .unwrap_or_else(|e| panic!("append: {e}"))
    }

    fn seqs(log: &dyn AuditLog, p: &str, range: AuditRange, limit: usize) -> Vec<u64> {
        log.list(&project(p), range, limit)
            .unwrap_or_else(|e| panic!("list: {e}"))
            .iter()
            .map(|e| e.seq)
            .collect()
    }

    /// What every `AuditLog` does; run against the fake and the store.
    fn contract(log: &dyn AuditLog) {
        let first = append(log, "p1", Some("s1"), true);
        append(log, "p2", None, true);
        let third = append(log, "p1", None, false);
        assert!(third.seq > first.seq);
        let all = log.list(&project("p1"), Default::default(), 10);
        assert_eq!(all, Ok(vec![first.clone(), third.clone()]));
        let after = |seq| AuditRange {
            after: Some(seq),
            ..AuditRange::default()
        };
        assert_eq!(seqs(log, "p1", after(first.seq), 10), [third.seq]);
        assert_eq!(seqs(log, "p1", AuditRange::default(), 1), [first.seq]);
        assert_eq!(
            seqs(log, "p3", AuditRange::default(), 10),
            Vec::<u64>::new()
        );
        // Newest first: the end of the log, then back from the lowest shown.
        let fourth = append(log, "p1", None, true);
        let newest = |before| AuditRange {
            before,
            order: AuditOrder::NewestFirst,
            ..AuditRange::default()
        };
        assert_eq!(seqs(log, "p1", newest(None), 2), [fourth.seq, third.seq]);
        assert_eq!(seqs(log, "p1", newest(Some(third.seq)), 2), [first.seq]);
        let between = AuditRange {
            after: Some(first.seq),
            before: Some(fourth.seq),
            order: AuditOrder::NewestFirst,
        };
        assert_eq!(seqs(log, "p1", between, 10), [third.seq]);
    }

    #[test]
    fn fake_and_store_keep_the_same_log() {
        contract(&MemoryAudit::default());
        let dir = TempDir::new().expect("temp dir");
        contract(&Store::open(dir.path()).expect("open"));
    }

    #[test]
    fn entries_outlive_a_restart_and_their_project() {
        let dir = TempDir::new().expect("temp dir");
        let root = dir.path().join("work");
        std::fs::create_dir(&root).expect("create folder");
        let store = Store::open(dir.path()).expect("open");
        let p = store.add(&root).expect("add");
        let first = append(&store, &p.id.0, Some("s1"), true);
        store.remove(&p.id).expect("remove");
        drop(store);
        let store = Store::open(dir.path()).expect("reopen");
        assert_eq!(
            AuditLog::list(&store, &p.id, Default::default(), 10),
            Ok(vec![first.clone()])
        );
        assert!(append(&store, &p.id.0, None, false).seq > first.seq);
    }

    #[test]
    fn entries_cannot_be_edited_or_deleted() {
        let dir = TempDir::new().expect("temp dir");
        let store = Store::open(dir.path()).expect("open");
        let entry = append(&store, "p1", None, true);
        let db = store.db();
        for sql in [
            "UPDATE audit SET event = '{}' WHERE seq = ?1",
            "DELETE FROM audit WHERE seq = ?1",
        ] {
            let err = db.execute(sql, [entry.seq]).err().map(|e| e.to_string());
            assert!(
                err.as_deref().is_some_and(|e| e.contains("append-only")),
                "{sql}: {err:?}"
            );
        }
        drop(db);
        assert_eq!(
            AuditLog::list(&store, &project("p1"), Default::default(), 10),
            Ok(vec![entry])
        );
    }

    #[cfg(unix)]
    #[test]
    fn only_the_user_may_read_the_database() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |dir: &TempDir| {
            std::fs::metadata(dir.path().join(crate::DB_FILE))
                .expect("database file")
                .permissions()
                .mode()
                & 0o777
        };
        let fresh = TempDir::new().expect("temp dir");
        drop(Store::open(fresh.path()).expect("open"));
        assert_eq!(mode(&fresh), 0o600);
        // A database from before this change is narrowed too.
        let old = TempDir::new().expect("temp dir");
        let file = old.path().join(crate::DB_FILE);
        std::fs::write(&file, "").expect("write");
        let wal = old.path().join(format!("{}-wal", crate::DB_FILE));
        for path in [&file, &wal] {
            std::fs::write(path, "").expect("write");
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644)).expect("chmod");
        }
        let store = Store::open(old.path()).expect("open");
        assert_eq!(mode(&old), 0o600);
        let wal_mode = std::fs::metadata(&wal).map(|m| m.permissions().mode() & 0o777);
        assert_eq!(wal_mode.ok(), Some(0o600));
        drop(store);
    }
}
