//! Persistent project list in SQLite.
//!
//! Contract: `ovra_core::services::ProjectStore`; the in-memory reference
//! behaviour is `ovra_core::fakes::MemoryProjects`. The database is one file,
//! `projects.sqlite3`, in the data directory the front end passes in. It
//! also keeps agent sessions (`SessionStore`, see `sessions.rs`) and the
//! audit log (`AuditLog`, see `audit.rs`), so only the user may read it.

mod audit;
mod sessions;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use ovra_core::protocol::agents::{AgentSettings, ProjectAgentConfig};
use ovra_core::protocol::netproxy::Allowlist;
use ovra_core::protocol::policy::Policy;
use ovra_core::protocol::projects::{Project, ProjectId, ProjectState, ProjectStatePatch};
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{ProjectStore, Result};
use rusqlite::{Connection, OptionalExtension, Row, params};

const DB_FILE: &str = "projects.sqlite3";

/// Schema migrations, applied in order; `PRAGMA user_version` counts how many
/// have run. Append new steps, never edit old ones.
const MIGRATIONS: &[&str] = &[
    "CREATE TABLE projects (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        root TEXT NOT NULL UNIQUE,
        added_ms INTEGER NOT NULL,
        last_opened_ms INTEGER
    ) STRICT;
    CREATE INDEX projects_by_last_opened ON projects (last_opened_ms DESC);",
    // Per-project UI state as JSON, so new fields need no migration.
    "CREATE TABLE project_state (
        project_id TEXT PRIMARY KEY REFERENCES projects (id) ON DELETE CASCADE,
        state TEXT NOT NULL
    ) STRICT;",
    // Whether the user trusts the project's agent configuration, and
    // app-wide settings as JSON per key.
    "ALTER TABLE projects ADD COLUMN trusted INTEGER NOT NULL DEFAULT 0;
    CREATE TABLE settings (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
    ) STRICT;",
    // What the user reviewed when they trusted the project, as JSON. Rows
    // trusted before this have none, so their settings count as changed.
    "ALTER TABLE projects ADD COLUMN trusted_config TEXT;",
    // Agent sessions (`SessionInfo` as JSON) and their transcripts, one
    // message per row in order.
    "CREATE TABLE agent_sessions (
        id TEXT PRIMARY KEY,
        project_id TEXT NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
        info TEXT NOT NULL,
        agent_session TEXT
    ) STRICT;
    CREATE TABLE agent_messages (
        id INTEGER PRIMARY KEY,
        session_id TEXT NOT NULL REFERENCES agent_sessions (id) ON DELETE CASCADE,
        message TEXT NOT NULL
    ) STRICT;
    CREATE INDEX agent_messages_by_session ON agent_messages (session_id, id);",
    // The project's policy (`docs/specs/security.md`) as JSON; none means
    // the empty policy, which asks about everything.
    "ALTER TABLE projects ADD COLUMN policy TEXT;",
    // The audit log (`docs/specs/security.md`): append-only, so the
    // triggers refuse edits and deletes, and no foreign key, so removing a
    // project keeps its entries. AUTOINCREMENT never reuses a `seq`.
    "CREATE TABLE audit (
        seq INTEGER PRIMARY KEY AUTOINCREMENT,
        at_ms INTEGER NOT NULL,
        project_id TEXT NOT NULL,
        session_id TEXT,
        event TEXT NOT NULL
    ) STRICT;
    CREATE INDEX audit_by_project ON audit (project_id, seq);
    CREATE TRIGGER audit_no_update BEFORE UPDATE ON audit
    BEGIN SELECT RAISE(ABORT, 'the audit log is append-only'); END;
    CREATE TRIGGER audit_no_delete BEFORE DELETE ON audit
    BEGIN SELECT RAISE(ABORT, 'the audit log is append-only'); END;",
    // The hosts the project's agents may reach (`netproxy.*`) as JSON; none
    // means the empty list.
    "ALTER TABLE projects ADD COLUMN allowlist TEXT;",
];

/// The `settings` key of [`AgentSettings`].
const AGENT_SETTINGS: &str = "agents";

const COLUMNS: &str = "id, name, root, last_opened_ms, trusted";

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
        let file = data_dir.join(DB_FILE);
        owner_only(&file)?;
        let mut db = Connection::open(&file).map_err(db_error)?;
        db.pragma_update(None, "journal_mode", "WAL")
            .map_err(db_error)?;
        db.pragma_update(None, "foreign_keys", true)
            .map_err(db_error)?;
        db.busy_timeout(std::time::Duration::from_secs(2))
            .map_err(db_error)?;
        migrate(&mut db)?;
        Ok(Self { db: Mutex::new(db) })
    }

    fn db(&self) -> MutexGuard<'_, Connection> {
        self.db.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A project's JSON column; the default if it was never set.
    fn json_column<T: serde::de::DeserializeOwned + Default>(
        &self,
        id: &ProjectId,
        column: Json,
    ) -> Result<T> {
        let json: Option<String> = self
            .db()
            .query_row(
                &format!("SELECT {} FROM projects WHERE id = ?1", column.name()),
                [&id.0],
                |row| row.get(0),
            )
            .optional()
            .map_err(db_error)?
            .ok_or_else(|| not_found(id))?;
        match json {
            None => Ok(T::default()),
            Some(json) => serde_json::from_str(&json)
                .map_err(|e| internal(format!("{} of project {}: {e}", column.name(), id.0))),
        }
    }

    fn set_json_column<T: serde::Serialize>(
        &self,
        id: &ProjectId,
        column: Json,
        value: &T,
    ) -> Result<()> {
        let json = serde_json::to_string(value).map_err(|e| internal(e.to_string()))?;
        let changed = self
            .db()
            .execute(
                &format!("UPDATE projects SET {} = ?2 WHERE id = ?1", column.name()),
                params![id.0, json],
            )
            .map_err(db_error)?;
        if changed == 0 {
            return Err(not_found(id));
        }
        Ok(())
    }
}

/// The project columns kept as JSON.
#[derive(Debug, Clone, Copy)]
enum Json {
    Policy,
    Allowlist,
}

impl Json {
    fn name(self) -> &'static str {
        match self {
            Self::Policy => "policy",
            Self::Allowlist => "allowlist",
        }
    }
}

/// Creates `file` readable and writable by the user alone (0600), or
/// narrows an existing one and its `-wal` and `-shm` files to that. SQLite
/// gives new journal files the database file's mode.
#[cfg(unix)]
fn owner_only(file: &Path) -> Result<()> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let failed = |e: std::io::Error| internal(format!("protecting {}: {e}", file.display()));
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(file)
        .map_err(failed)?;
    std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o600)).map_err(failed)?;
    // Journal files left by an earlier run keep the mode they were made with.
    for suffix in ["-wal", "-shm"] {
        let mut journal = file.as_os_str().to_owned();
        journal.push(suffix);
        let journal = Path::new(&journal);
        if journal.exists() {
            std::fs::set_permissions(journal, std::fs::Permissions::from_mode(0o600))
                .map_err(failed)?;
        }
    }
    Ok(())
}

/// Windows: the file inherits the ACL of the user's own data folder.
#[cfg(not(unix))]
fn owner_only(_: &Path) -> Result<()> {
    Ok(())
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

    fn state(&self, id: &ProjectId) -> Result<ProjectState> {
        load_state(&self.db(), id)
    }

    fn update_state(&self, id: &ProjectId, patch: ProjectStatePatch) -> Result<()> {
        // Read, merge and write under one lock so concurrent patches from
        // different panels cannot lose each other's fields.
        let db = self.db();
        let mut state = load_state(&db, id)?;
        patch.apply(&mut state);
        let json = serde_json::to_string(&state).map_err(|e| internal(e.to_string()))?;
        db.execute(
            "INSERT INTO project_state (project_id, state) VALUES (?1, ?2)
             ON CONFLICT (project_id) DO UPDATE SET state = excluded.state",
            params![id.0, json],
        )
        .map_err(db_error)?;
        Ok(())
    }

    fn set_trusted(&self, id: &ProjectId, trusted: Option<ProjectAgentConfig>) -> Result<Project> {
        let config = trusted
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| internal(e.to_string()))?;
        self.db()
            .query_row(
                &format!(
                    "UPDATE projects SET trusted = ?2, trusted_config = ?3 WHERE id = ?1
                     RETURNING {COLUMNS}"
                ),
                params![id.0, trusted.is_some(), config],
                project_from_row,
            )
            .optional()
            .map_err(db_error)?
            .ok_or_else(|| not_found(id))
    }

    fn trusted_config(&self, id: &ProjectId) -> Result<Option<ProjectAgentConfig>> {
        let (trusted, config): (bool, Option<String>) = self
            .db()
            .query_row(
                "SELECT trusted, trusted_config FROM projects WHERE id = ?1",
                [&id.0],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(db_error)?
            .ok_or_else(|| not_found(id))?;
        if !trusted {
            return Ok(None);
        }
        let Some(config) = config else {
            return Ok(Some(ProjectAgentConfig::default()));
        };
        serde_json::from_str(&config)
            .map(Some)
            .map_err(|e| internal(format!("trusted settings of project {}: {e}", id.0)))
    }

    fn policy(&self, id: &ProjectId) -> Result<Policy> {
        self.json_column(id, Json::Policy)
    }

    fn set_policy(&self, id: &ProjectId, policy: &Policy) -> Result<()> {
        self.set_json_column(id, Json::Policy, policy)
    }

    fn allowlist(&self, id: &ProjectId) -> Result<Allowlist> {
        self.json_column(id, Json::Allowlist)
    }

    fn set_allowlist(&self, id: &ProjectId, allowlist: &Allowlist) -> Result<()> {
        self.set_json_column(id, Json::Allowlist, allowlist)
    }

    fn agent_settings(&self) -> Result<AgentSettings> {
        let json: Option<String> = self
            .db()
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                [AGENT_SETTINGS],
                |row| row.get(0),
            )
            .optional()
            .map_err(db_error)?;
        json.map_or_else(
            || Ok(AgentSettings::default()),
            |json| {
                serde_json::from_str(&json).map_err(|e| internal(format!("agent settings: {e}")))
            },
        )
    }

    fn set_agent_settings(&self, settings: AgentSettings) -> Result<()> {
        let json = serde_json::to_string(&settings).map_err(|e| internal(e.to_string()))?;
        self.db()
            .execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                params![AGENT_SETTINGS, json],
            )
            .map_err(db_error)?;
        Ok(())
    }
}

/// The saved state, the default if none was saved, `not_found` without a project.
fn load_state(db: &Connection, id: &ProjectId) -> Result<ProjectState> {
    find(db, id)?;
    let json: Option<String> = db
        .query_row(
            "SELECT state FROM project_state WHERE project_id = ?1",
            [&id.0],
            |row| row.get(0),
        )
        .optional()
        .map_err(db_error)?;
    json.map_or_else(
        || Ok(ProjectState::default()),
        |json| {
            serde_json::from_str(&json)
                .map_err(|e| internal(format!("saved state of project {}: {e}", id.0)))
        },
    )
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
        trusted: row.get(4)?,
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
