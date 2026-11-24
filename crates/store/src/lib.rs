//! Persistent project list in SQLite.
//!
//! Contract: `ovra_core::services::ProjectStore`; the in-memory reference
//! behaviour is `ovra_core::fakes::MemoryProjects`. The database is one file,
//! `projects.sqlite3`, in the data directory the front end passes in.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use ovra_core::protocol::projects::{Project, ProjectId, ProjectState};
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{ProjectStore, Result};
use rusqlite::{Connection, OptionalExtension, Row, params};

const DB_FILE: &str = "projects.sqlite3";

/// Schema migrations, applied in order; `PRAGMA user_version` counts how many
/// have run. Append new steps, never edit old ones.
const MIGRATIONS: &[&str] = &["CREATE TABLE projects (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        root TEXT NOT NULL UNIQUE,
        added_ms INTEGER NOT NULL,
        last_opened_ms INTEGER
    ) STRICT;
    CREATE INDEX projects_by_last_opened ON projects (last_opened_ms DESC);"];

const COLUMNS: &str = "id, name, root, last_opened_ms";

/// The real project store, kept in `data_dir`.
#[derive(Debug)]
pub struct Store {
    db: Mutex<Connection>,
}

impl Store {
    /// Opens (creating if needed) the store in `data_dir`.
    pub fn open(data_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(data_dir).map_err(|e| {
            internal(format!(
                "creating data directory {}: {e}",
                data_dir.display()
            ))
        })?;
        let mut db = Connection::open(data_dir.join(DB_FILE)).map_err(db_error)?;
        db.pragma_update(None, "journal_mode", "WAL")
            .map_err(db_error)?;
        db.busy_timeout(std::time::Duration::from_secs(2))
            .map_err(db_error)?;
        migrate(&mut db)?;
        Ok(Self { db: Mutex::new(db) })
    }

    fn db(&self) -> MutexGuard<'_, Connection> {
        self.db.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn migrate(db: &mut Connection) -> Result<()> {
    let applied: usize = db
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(db_error)?;
    if applied > MIGRATIONS.len() {
        return Err(internal(format!(
            "the project database has schema version {applied}, newer than this build supports ({})",
            MIGRATIONS.len()
        )));
    }
    for (version, sql) in MIGRATIONS.iter().enumerate().skip(applied) {
        let tx = db.transaction().map_err(db_error)?;
        tx.execute_batch(sql).map_err(db_error)?;
        tx.pragma_update(None, "user_version", version + 1)
            .map_err(db_error)?;
        tx.commit().map_err(db_error)?;
    }
    Ok(())
}

impl ProjectStore for Store {
    fn list(&self) -> Result<Vec<Project>> {
        let db = self.db();
        let mut stmt = db
            .prepare_cached(&format!(
                "SELECT {COLUMNS} FROM projects
                 ORDER BY last_opened_ms IS NULL, last_opened_ms DESC, added_ms, rowid"
            ))
            .map_err(db_error)?;
        let rows = stmt.query_map([], project_from_row).map_err(db_error)?;
        rows.collect::<rusqlite::Result<_>>().map_err(db_error)
    }

    fn add(&self, root: &Path) -> Result<Project> {
        let root = canonical_dir(root)?;
        let root_text = root.to_string_lossy().into_owned();
        let name = root
            .file_name()
            .map_or_else(|| root_text.clone(), |n| n.to_string_lossy().into_owned());
        let db = self.db();
        // A unique root makes this idempotent: an existing row wins.
        db.execute(
            "INSERT INTO projects (id, name, root, added_ms)
             VALUES (lower(hex(randomblob(8))), ?1, ?2, ?3)
             ON CONFLICT (root) DO NOTHING",
            params![name, root_text, now_ms()],
        )
        .map_err(db_error)?;
        db.query_row(
            &format!("SELECT {COLUMNS} FROM projects WHERE root = ?1"),
            [&root_text],
            project_from_row,
        )
        .map_err(db_error)
    }

    fn remove(&self, id: &ProjectId) -> Result<()> {
        let removed = self
            .db()
            .execute("DELETE FROM projects WHERE id = ?1", [&id.0])
            .map_err(db_error)?;
        if removed == 0 {
            return Err(not_found(id));
        }
        Ok(())
    }

    fn get(&self, id: &ProjectId) -> Result<Project> {
        find(&self.db(), id)
    }

    fn touch(&self, id: &ProjectId) -> Result<Project> {
        // Strictly increasing, so the latest touch sorts first even when the
        // wall clock is coarse or steps back.
        self.db()
            .query_row(
                &format!(
                    "UPDATE projects
                     SET last_opened_ms = max(?2, (SELECT coalesce(max(last_opened_ms), 0) + 1 FROM projects))
                     WHERE id = ?1
                     RETURNING {COLUMNS}"
                ),
                params![id.0, now_ms()],
                project_from_row,
            )
            .optional()
            .map_err(db_error)?
            .ok_or_else(|| not_found(id))
    }

    fn state(&self, _id: &ProjectId) -> Result<ProjectState> {
        Err(ProtocolError::new(
            ErrorCode::Unavailable,
            "project state is not stored yet",
        ))
    }

    fn set_state(&self, _id: &ProjectId, _state: &ProjectState) -> Result<()> {
        Err(ProtocolError::new(
            ErrorCode::Unavailable,
            "project state is not stored yet",
        ))
    }
}

fn find(db: &Connection, id: &ProjectId) -> Result<Project> {
    db.query_row(
        &format!("SELECT {COLUMNS} FROM projects WHERE id = ?1"),
        [&id.0],
        project_from_row,
    )
    .optional()
    .map_err(db_error)?
    .ok_or_else(|| not_found(id))
}

fn project_from_row(row: &Row<'_>) -> rusqlite::Result<Project> {
    Ok(Project {
        id: ProjectId(row.get(0)?),
        name: row.get(1)?,
        root: row.get(2)?,
        last_opened_ms: row.get(3)?,
    })
}

/// Resolves symlinks and `..` so one folder is one project.
fn canonical_dir(root: &Path) -> Result<PathBuf> {
    let canonical = root.canonicalize().map_err(|e| {
        let code = if e.kind() == std::io::ErrorKind::NotFound {
            ErrorCode::NotFound
        } else {
            ErrorCode::BadRequest
        };
        ProtocolError::new(code, format!("{}: {e}", root.display()))
    })?;
    if !canonical.is_dir() {
        return Err(ProtocolError::new(
            ErrorCode::BadRequest,
            format!("{} is not a folder", root.display()),
        ));
    }
    Ok(canonical)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

fn not_found(id: &ProjectId) -> ProtocolError {
    ProtocolError::new(ErrorCode::NotFound, format!("project {} not found", id.0))
}

fn internal(message: String) -> ProtocolError {
    ProtocolError::new(ErrorCode::Internal, message)
}

fn db_error(e: rusqlite::Error) -> ProtocolError {
    internal(format!("project database: {e}"))
}

#[cfg(test)]
mod tests;
