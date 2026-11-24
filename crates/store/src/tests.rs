use std::sync::Arc;

use ovra_core::Core;
use ovra_core::fakes::{MemoryFs, MemoryProjects};
use ovra_core::protocol::projects::{ProjectsRequest, ProjectsResponse};
use ovra_core::protocol::{Request, Response};
use tempfile::TempDir;

use super::*;

/// Folders `a`, `b`, `c` under a temporary directory.
struct Folders {
    dir: TempDir,
}

impl Folders {
    fn new() -> Self {
        let dir = TempDir::new().expect("temp dir");
        for name in ["a", "b", "c"] {
            std::fs::create_dir(dir.path().join(name)).expect("create folder");
        }
        Self { dir }
    }

    /// Canonical, so the fake (which keeps paths as given) and the store
    /// (which canonicalizes) agree on roots.
    fn path(&self, name: &str) -> PathBuf {
        self.dir
            .path()
            .canonicalize()
            .expect("canonical temp dir")
            .join(name)
    }
}

fn names(list: &[Project]) -> Vec<&str> {
    list.iter().map(|p| p.name.as_str()).collect()
}

/// The behaviour every `ProjectStore` must have; run against the fake and
/// the real store so they cannot drift apart.
fn contract(store: &dyn ProjectStore, folders: &Folders) {
    assert_eq!(store.list(), Ok(Vec::new()));

    let a = store.add(&folders.path("a")).expect("add a");
    assert_eq!(a.name, "a");
    assert_eq!(a.root, folders.path("a").to_string_lossy());
    assert_eq!(a.last_opened_ms, None);
    assert_eq!(
        store.add(&folders.path("a")),
        Ok(a.clone()),
        "add is idempotent"
    );

    let b = store.add(&folders.path("b")).expect("add b");
    let c = store.add(&folders.path("c")).expect("add c");
    assert_ne!(a.id, b.id);
    assert_eq!(store.get(&b.id), Ok(b.clone()));

    // Most recently opened first, never-opened ones last.
    store.touch(&b.id).expect("touch b");
    let touched_a = store.touch(&a.id).expect("touch a");
    assert!(touched_a.last_opened_ms.is_some());
    assert_eq!(names(&store.list().expect("list")), ["a", "b", "c"]);
    store.touch(&c.id).expect("touch c");
    assert_eq!(names(&store.list().expect("list")), ["c", "a", "b"]);

    // Per-project state: empty until saved; patches write only their fields.
    assert_eq!(store.state(&a.id), Ok(ProjectState::default()));
    let tree = ProjectStatePatch {
        expanded_dirs: Some(vec!["src".into()]),
        ..ProjectStatePatch::default()
    };
    let tabs = ProjectStatePatch {
        open_files: Some(vec!["src/main.rs".into(), "README.md".into()]),
        active_file: Some(Some("README.md".into())),
        ..ProjectStatePatch::default()
    };
    store.update_state(&a.id, tree).expect("save tree");
    store.update_state(&a.id, tabs).expect("save tabs");
    store
        .update_state(&b.id, ProjectStatePatch::default())
        .expect("empty patch");
    assert_eq!(store.state(&a.id), Ok(sample_state()));
    assert_eq!(store.state(&b.id), Ok(ProjectState::default()));
    let clear_active = ProjectStatePatch {
        active_file: Some(None),
        ..ProjectStatePatch::default()
    };
    store
        .update_state(&a.id, clear_active)
        .expect("clear active");
    assert_eq!(
        store.state(&a.id),
        Ok(ProjectState {
            active_file: None,
            ..sample_state()
        })
    );

    // Trust: off until set, kept with what was trusted, revocable.
    assert!(!a.trusted);
    assert_eq!(store.trusted_config(&a.id), Ok(None));
    let reviewed = reviewed();
    let trusted = store.set_trusted(&a.id, Some(reviewed.clone()));
    assert!(trusted.expect("trust a").trusted);
    assert!(store.get(&a.id).expect("get a").trusted);
    assert_eq!(store.trusted_config(&a.id), Ok(Some(reviewed.clone())));
    assert!(!store.get(&b.id).expect("get b").trusted);
    assert!(!store.set_trusted(&a.id, None).expect("revoke a").trusted);
    assert_eq!(store.trusted_config(&a.id), Ok(None));
    store
        .set_trusted(&a.id, Some(reviewed))
        .expect("trust a again");

    // Agent settings: defaults until saved.
    assert_eq!(store.agent_settings(), Ok(AgentSettings::default()));
    let settings = AgentSettings {
        claude_path: Some("/opt/claude".into()),
    };
    store
        .set_agent_settings(settings.clone())
        .expect("save settings");
    assert_eq!(store.agent_settings(), Ok(settings));

    store.remove(&a.id).expect("remove a");
    assert_eq!(names(&store.list().expect("list")), ["c", "b"]);
    // A folder added again starts untrusted.
    let again = store.add(&folders.path("a")).expect("add a again");
    assert!(!again.trusted);
    store.remove(&again.id).expect("remove a again");

    let gone = a.id;
    for result in [
        store.get(&gone).map(drop),
        store.touch(&gone).map(drop),
        store.state(&gone).map(drop),
        store.update_state(&gone, ProjectStatePatch::default()),
        store.set_trusted(&gone, None).map(drop),
        store.trusted_config(&gone).map(drop),
        store.remove(&gone),
    ] {
        assert_eq!(result.map_err(|e| e.code), Err(ErrorCode::NotFound));
    }
}

#[test]
fn fake_meets_the_contract() {
    contract(&MemoryProjects::default(), &Folders::new());
}

#[test]
fn store_meets_the_contract() {
    let data = TempDir::new().expect("temp dir");
    contract(&Store::open(data.path()).expect("open"), &Folders::new());
}

#[test]
fn projects_and_order_survive_a_restart() {
    let (data, folders) = (TempDir::new().expect("temp dir"), Folders::new());
    let before = {
        let store = Store::open(data.path()).expect("open");
        let a = store.add(&folders.path("a")).expect("add a");
        let b = store.add(&folders.path("b")).expect("add b");
        store.set_trusted(&b.id, Some(reviewed())).expect("trust b");
        store
            .set_agent_settings(AgentSettings {
                claude_path: Some("/opt/claude".into()),
            })
            .expect("save settings");
        store.touch(&a.id).expect("touch a");
        store.touch(&b.id).expect("touch b");
        store
            .update_state(&a.id, full(sample_state()))
            .expect("save state");
        store.list().expect("list")
    };
    let store = Store::open(data.path()).expect("reopen");
    assert_eq!(store.state(&before[1].id), Ok(sample_state()));
    assert!(before[0].trusted);
    assert_eq!(store.trusted_config(&before[0].id), Ok(Some(reviewed())));
    assert_eq!(store.list(), Ok(before));
    let path = store.agent_settings().expect("settings").claude_path;
    assert_eq!(path.as_deref(), Some("/opt/claude"));
}

fn reviewed() -> ProjectAgentConfig {
    use ovra_core::protocol::agents::{ConfigItem, ConfigKind};
    ProjectAgentConfig {
        fingerprint: "f1".into(),
        items: vec![ConfigItem {
            path: ".claude/settings.json".into(),
            kind: ConfigKind::Hooks,
            entries: vec!["Stop: ./done.sh".into()],
        }],
    }
}

fn sample_state() -> ProjectState {
    ProjectState {
        open_files: vec!["src/main.rs".into(), "README.md".into()],
        active_file: Some("README.md".into()),
        expanded_dirs: vec!["src".into()],
    }
}

fn full(state: ProjectState) -> ProjectStatePatch {
    ProjectStatePatch {
        open_files: Some(state.open_files),
        active_file: Some(state.active_file),
        expanded_dirs: Some(state.expanded_dirs),
    }
}

#[test]
fn removing_a_project_drops_its_state() {
    let (data, folders) = (TempDir::new().expect("temp dir"), Folders::new());
    let store = Store::open(data.path()).expect("open");
    let a = store.add(&folders.path("a")).expect("add a");
    store
        .update_state(&a.id, full(sample_state()))
        .expect("save state");
    store.remove(&a.id).expect("remove a");
    let rows: u32 = store
        .db()
        .query_row("SELECT count(*) FROM project_state", [], |row| row.get(0))
        .expect("count");
    assert_eq!(rows, 0);
    let again = store.add(&folders.path("a")).expect("add a again");
    assert_eq!(store.state(&again.id), Ok(ProjectState::default()));
}

#[test]
fn saved_state_tolerates_missing_and_unknown_fields() {
    let (data, folders) = (TempDir::new().expect("temp dir"), Folders::new());
    let store = Store::open(data.path()).expect("open");
    let a = store.add(&folders.path("a")).expect("add a");
    store
        .db()
        .execute(
            "INSERT INTO project_state (project_id, state) VALUES (?1, ?2)",
            params![a.id.0, r#"{"open_files": ["a.rs"], "gone_field": 1}"#],
        )
        .expect("insert");
    let state = store.state(&a.id).expect("state");
    assert_eq!(state.open_files, ["a.rs"]);
    assert_eq!(state.active_file, None);
}

#[test]
fn data_dir_is_created() {
    let data = TempDir::new().expect("temp dir");
    let nested = data.path().join("x/y");
    Store::open(&nested).expect("open");
    assert!(nested.join(DB_FILE).is_file());
}

#[test]
fn one_folder_is_one_project() {
    let (data, folders) = (TempDir::new().expect("temp dir"), Folders::new());
    let store = Store::open(data.path()).expect("open");
    let a = store.add(&folders.path("a")).expect("add a");
    let roundabout = folders.path("b").join("../a/");
    assert_eq!(store.add(&roundabout), Ok(a.clone()));
    #[cfg(unix)]
    {
        let link = folders.path("link");
        std::os::unix::fs::symlink(folders.path("a"), &link).expect("symlink");
        assert_eq!(store.add(&link), Ok(a));
    }
}

#[test]
fn only_existing_folders_can_be_added() {
    let (data, folders) = (TempDir::new().expect("temp dir"), Folders::new());
    let store = Store::open(data.path()).expect("open");
    let missing = store.add(&folders.path("missing"));
    assert_eq!(missing.map_err(|e| e.code), Err(ErrorCode::NotFound));
    let file = folders.path("file.txt");
    std::fs::write(&file, "x").expect("write file");
    assert_eq!(
        store.add(&file).map_err(|e| e.code),
        Err(ErrorCode::BadRequest)
    );
    assert_eq!(store.list(), Ok(Vec::new()));
}

#[test]
fn touch_is_strictly_increasing() {
    let (data, folders) = (TempDir::new().expect("temp dir"), Folders::new());
    let store = Store::open(data.path()).expect("open");
    let a = store.add(&folders.path("a")).expect("add a");
    let b = store.add(&folders.path("b")).expect("add b");
    // Far in the future, as if the clock stepped back afterwards.
    store
        .db()
        .execute(
            "UPDATE projects SET last_opened_ms = ?1 WHERE id = ?2",
            params![u64::from(u32::MAX) * 1000, a.id.0],
        )
        .expect("set time");
    let b = store.touch(&b.id).expect("touch b");
    assert_eq!(b.last_opened_ms, Some(u64::from(u32::MAX) * 1000 + 1));
    assert_eq!(names(&store.list().expect("list")), ["b", "a"]);
}

#[test]
fn a_newer_schema_is_refused() {
    let data = TempDir::new().expect("temp dir");
    drop(Store::open(data.path()).expect("open"));
    Connection::open(data.path().join(DB_FILE))
        .and_then(|db| db.pragma_update(None, "user_version", MIGRATIONS.len() + 1))
        .expect("bump version");
    let err = Store::open(data.path()).expect_err("must refuse");
    assert_eq!(err.code, ErrorCode::Internal);
    assert!(err.message.contains("newer"), "{}", err.message);
}

#[tokio::test]
async fn switching_projects_through_the_core_is_remembered() {
    let (data, folders) = (TempDir::new().expect("temp dir"), Folders::new());
    let fs = Arc::new(MemoryFs::default());
    let core = Core::builder("test")
        .projects(Arc::new(Store::open(data.path()).expect("open")))
        .fs(fs.clone())
        .build();
    let call = async |request| match core.handle(Request::Projects(request)).await {
        Ok(Response::Projects(response)) => response,
        other => panic!("unexpected {other:?}"),
    };
    let mut ids = Vec::new();
    for name in ["a", "b"] {
        let root = folders.path(name).to_string_lossy().into_owned();
        match call(ProjectsRequest::Add { root }).await {
            ProjectsResponse::Project(p) => ids.push(p.id),
            other => panic!("unexpected {other:?}"),
        }
    }
    for id in [&ids[0], &ids[1], &ids[0]] {
        call(ProjectsRequest::Open { id: id.clone() }).await;
    }
    assert_eq!(fs.watched_ids(), [ids[0].clone()]);

    // A fresh store over the same data sees the same order.
    let reopened = Store::open(data.path()).expect("reopen");
    assert_eq!(names(&reopened.list().expect("list")), ["a", "b"]);
}

#[test]
fn a_database_from_the_first_schema_is_upgraded() {
    let (data, folders) = (TempDir::new().expect("temp dir"), Folders::new());
    let root = folders.path("a").to_string_lossy().into_owned();
    {
        let db = Connection::open(data.path().join(DB_FILE)).expect("create");
        db.execute_batch(MIGRATIONS[0]).expect("first schema");
        db.pragma_update(None, "user_version", 1).expect("version");
        db.execute(
            "INSERT INTO projects (id, name, root, added_ms) VALUES ('old', 'a', ?1, 1)",
            [&root],
        )
        .expect("insert");
    }
    let store = Store::open(data.path()).expect("upgrade");
    let id = ProjectId("old".into());
    store
        .update_state(&id, full(sample_state()))
        .expect("save state");
    assert_eq!(store.state(&id), Ok(sample_state()));
    assert!(!store.get(&id).expect("get").trusted);
}

#[test]
fn a_project_trusted_before_trust_kept_its_settings_counts_as_changed() {
    let (data, folders) = (TempDir::new().expect("temp dir"), Folders::new());
    let root = folders.path("a").to_string_lossy().into_owned();
    {
        let db = Connection::open(data.path().join(DB_FILE)).expect("create");
        for sql in &MIGRATIONS[..3] {
            db.execute_batch(sql).expect("old schema");
        }
        db.pragma_update(None, "user_version", 3).expect("version");
        db.execute(
            "INSERT INTO projects (id, name, root, added_ms, trusted) VALUES ('old', 'a', ?1, 1, 1)",
            [&root],
        )
        .expect("insert");
    }
    let store = Store::open(data.path()).expect("upgrade");
    let config = store.trusted_config(&ProjectId("old".into()));
    assert_eq!(config, Ok(Some(ProjectAgentConfig::default())));
}

#[test]
fn a_projects_policy_is_kept_across_opens() {
    use ovra_core::protocol::policy::{Policy, PolicyAction, PolicyRule};

    let (data, folders) = (TempDir::new().expect("temp dir"), Folders::new());
    let store = Store::open(data.path()).expect("open");
    let a = store.add(&folders.path("a")).expect("add a");
    assert_eq!(store.policy(&a.id), Ok(Policy::default()));
    let policy = Policy {
        rules: vec![PolicyRule {
            tool: "Bash".into(),
            pattern: Some("npm test".into()),
            action: PolicyAction::Auto,
        }],
    };
    store.set_policy(&a.id, &policy).expect("set");
    drop(store);
    let store = Store::open(data.path()).expect("reopen");
    assert_eq!(store.policy(&a.id), Ok(policy.clone()));
    let gone = ProjectId("p9".into());
    assert_eq!(
        store.policy(&gone).map_err(|e| e.code),
        Err(ErrorCode::NotFound)
    );
    assert_eq!(
        store.set_policy(&gone, &policy).map_err(|e| e.code),
        Err(ErrorCode::NotFound)
    );
}
