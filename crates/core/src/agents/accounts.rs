//! Claude accounts: configuration folders with their own sign-in, one per
//! account, which `claude` gets as `CLAUDE_CONFIG_DIR` (ADR 0006). The core
//! keeps the list and decides which account a session runs with; the
//! runtime checks each one.

use std::path::{Component, Path, PathBuf};

use super::requests::Ctx;
use crate::protocol::agents::{
    AccountState, AccountsOverview, AgentSettings, ClaudeAccount, DEFAULT_ACCOUNT,
};
use crate::protocol::projects::ProjectId;
use crate::protocol::{ErrorCode, ProtocolError};

type Result<T> = std::result::Result<T, ProtocolError>;

/// Longest account name, in characters.
const MAX_NAME: usize = 60;

/// What a session runs with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Chosen {
    /// `None` for the default account.
    pub(crate) id: Option<String>,
    pub(crate) executable: Option<String>,
    pub(crate) config_dir: Option<String>,
}

/// The account a session of `project` runs with: `explicit` if given (it
/// must exist), else the project's, else the default one. A project's
/// account that was removed meanwhile counts as unset.
pub(crate) fn choose(
    settings: &AgentSettings,
    project: &ProjectId,
    explicit: Option<&str>,
) -> Result<Chosen> {
    let account = match explicit {
        Some(id) => settings.account(Some(id)).ok_or_else(|| missing(id))?,
        None => settings
            .project_accounts
            .get(project)
            .and_then(|id| settings.account(Some(id)))
            .unwrap_or_else(|| {
                settings
                    .account(None)
                    .unwrap_or_else(ClaudeAccount::default_account)
            }),
    };
    Ok(Chosen {
        executable: settings.claude_path.clone(),
        config_dir: account.config_dir.clone(),
        id: (account.id != DEFAULT_ACCOUNT).then_some(account.id),
    })
}

fn missing(id: &str) -> ProtocolError {
    ProtocolError::new(
        ErrorCode::NotFound,
        format!("there is no Claude account {id:?}; it may have been removed"),
    )
}

/// Every account checked by the runtime, the folders it found that no
/// account uses, and `project`'s account.
pub(crate) async fn overview(
    ctx: &Ctx<'_>,
    project: Option<&ProjectId>,
) -> Result<AccountsOverview> {
    let settings = ctx.projects.agent_settings()?;
    if let Some(project) = project {
        ctx.projects.get(project)?;
    }
    let mut accounts = Vec::new();
    for account in settings.all_accounts() {
        let setup = ctx
            .runtime
            .setup(
                settings.claude_path.clone().map(PathBuf::from),
                account.config_dir.as_ref().map(PathBuf::from),
            )
            .await;
        accounts.push(AccountState { account, setup });
    }
    let used: Vec<PathBuf> = accounts
        .iter()
        .filter_map(|a| a.account.config_dir.as_ref().map(PathBuf::from))
        .collect();
    let found = ctx
        .runtime
        .config_dirs()
        .await
        .into_iter()
        .filter(|dir| !used.contains(dir))
        .map(|dir| dir.to_string_lossy().into_owned())
        .collect();
    let project_account = project
        .and_then(|p| settings.project_accounts.get(p))
        .filter(|id| settings.account(Some(id)).is_some())
        .cloned();
    Ok(AccountsOverview {
        accounts,
        found,
        project_account,
    })
}

/// Adds `account` (an empty id) or renames the one with its id: an
/// account's folder holds its conversations, so it never changes.
pub(crate) fn save(settings: &mut AgentSettings, mut account: ClaudeAccount) -> Result<()> {
    account.name = account.name.trim().to_owned();
    if account.name.is_empty() {
        return Err(bad("the account needs a name"));
    }
    if account.name.chars().count() > MAX_NAME {
        return Err(bad(&format!(
            "the account name is longer than {MAX_NAME} characters"
        )));
    }
    account.config_dir = folder(account.config_dir.as_deref())?;
    let default = account.id == DEFAULT_ACCOUNT;
    if default && account.config_dir.is_some() {
        return Err(bad(
            "the default account uses Claude Code's own folder; add another account for a different folder",
        ));
    }
    if !default && account.config_dir.is_none() {
        return Err(bad("the account needs its configuration folder"));
    }
    if let Some(dir) = &account.config_dir {
        let taken = settings
            .all_accounts()
            .into_iter()
            .find(|a| a.id != account.id && a.config_dir.as_ref() == Some(dir));
        if let Some(other) = taken {
            return Err(ProtocolError::new(
                ErrorCode::Conflict,
                format!("the account {:?} already uses {dir}", other.name),
            ));
        }
    }
    if account.id.is_empty() {
        account.id = new_id(settings);
        settings.accounts.push(account);
        return Ok(());
    }
    if let Some(saved) = settings.accounts.iter_mut().find(|a| a.id == account.id) {
        if saved.config_dir != account.config_dir {
            return Err(bad(
                "an account's folder cannot change: its sessions live there; add another account instead",
            ));
        }
        *saved = account;
    } else if default {
        settings.accounts.insert(0, account);
    } else {
        return Err(missing(&account.id));
    }
    Ok(())
}

/// Removes the account `id`; projects that used it go back to the default.
pub(crate) fn remove(settings: &mut AgentSettings, id: &str) -> Result<()> {
    if id == DEFAULT_ACCOUNT {
        return Err(bad("the default account cannot be removed"));
    }
    let before = settings.accounts.len();
    settings.accounts.retain(|a| a.id != id);
    if settings.accounts.len() == before {
        return Err(missing(id));
    }
    settings.project_accounts.retain(|_, account| account != id);
    Ok(())
}

/// Sets `project`'s account; `None` or the default account clears it.
pub(crate) fn set_project(
    settings: &mut AgentSettings,
    project: &ProjectId,
    account: Option<String>,
) -> Result<()> {
    match account.filter(|id| id != DEFAULT_ACCOUNT) {
        Some(id) => {
            settings.account(Some(&id)).ok_or_else(|| missing(&id))?;
            settings.project_accounts.insert(project.clone(), id);
        }
        None => {
            settings.project_accounts.remove(project);
        }
    }
    Ok(())
}

/// `account-1`, `account-2`, ...: the first number no account has.
fn new_id(settings: &AgentSettings) -> String {
    (1..)
        .map(|n| format!("account-{n}"))
        .find(|id| settings.accounts.iter().all(|a| &a.id != id))
        .unwrap_or_default()
}

/// A configuration folder the user typed: empty is none; otherwise a full
/// path without `.` or `..` whose folder name starts with `.claude` (not
/// `~/.claude` itself, the default account's), such as
/// `/Users/you/.claude-work`. Any other folder could hold the person's
/// secrets or a project an agent writes, and `claude` runs with it. The
/// sandbox also checks that it sits right in the home folder.
fn folder(path: Option<&str>) -> Result<Option<String>> {
    let Some(path) = path.map(str::trim).filter(|p| !p.is_empty()) else {
        return Ok(None);
    };
    let path = path.trim_end_matches(['/', '\\']);
    let parsed = Path::new(path);
    if !parsed.is_absolute() {
        return Err(bad(&format!(
            "the configuration folder must be a full path, such as /Users/you/.claude-work, not {path}"
        )));
    }
    let tidy = parsed
        .components()
        .all(|c| !matches!(c, Component::CurDir | Component::ParentDir));
    let name = parsed
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !tidy || !name.starts_with(".claude") || name == ".claude" {
        return Err(bad(&format!(
            "{path} is not a Claude account's folder: use the folder in your home folder whose name starts with .claude, such as /Users/you/.claude-work"
        )));
    }
    Ok(Some(path.to_owned()))
}

fn bad(message: &str) -> ProtocolError {
    ProtocolError::new(ErrorCode::BadRequest, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn work() -> ClaudeAccount {
        ClaudeAccount {
            id: String::new(),
            name: " Work ".into(),
            config_dir: Some(crate::test_path!("/home/me/.claude-work/").into()),
        }
    }

    #[test]
    fn an_added_account_gets_an_id_and_a_clean_folder() {
        let mut settings = AgentSettings::default();
        save(&mut settings, work()).unwrap_or_else(|e| panic!("{e:?}"));
        let all = settings.all_accounts();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0], ClaudeAccount::default_account());
        assert_eq!(all[1].id, "account-1");
        assert_eq!(all[1].name, "Work");
        assert_eq!(
            all[1].config_dir.as_deref(),
            Some(crate::test_path!("/home/me/.claude-work"))
        );
        let mut again = work();
        again.config_dir = Some(crate::test_path!("/home/me/.claude-other").into());
        save(&mut settings, again).unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(settings.accounts[1].id, "account-2");
    }

    #[test]
    fn bad_accounts_are_refused() {
        let mut settings = AgentSettings::default();
        let refused = |settings: &mut AgentSettings, account: ClaudeAccount| {
            save(settings, account).err().map(|e| e.message)
        };
        let mut relative = work();
        relative.config_dir = Some("~/.claude-work".into());
        assert!(refused(&mut settings, relative).is_some_and(|m| m.contains("full path")));
        let mut up = work();
        up.config_dir = Some(crate::test_path!("/home/me/../other").into());
        assert!(refused(&mut settings, up).is_some());
        // Only a `.claude…` folder: never one that may hold secrets or a
        // project, and not the default account's own.
        for path in [
            crate::test_path!("/"),
            crate::test_path!("/home/me"),
            crate::test_path!("/home/me/Documents"),
            crate::test_path!("/home/me/proj/x"),
            crate::test_path!("/home/me/.ssh"),
            crate::test_path!("/home/me/.claude"),
        ] {
            let mut odd = work();
            odd.config_dir = Some(path.into());
            assert!(
                refused(&mut settings, odd).is_some_and(|m| m.contains(".claude")),
                "{path}"
            );
        }
        let mut none = work();
        none.config_dir = None;
        assert!(refused(&mut settings, none).is_some_and(|m| m.contains("folder")));
        let mut nameless = work();
        nameless.name = "  ".into();
        assert!(refused(&mut settings, nameless).is_some());
        let mut default = work();
        default.id = DEFAULT_ACCOUNT.into();
        assert!(refused(&mut settings, default).is_some_and(|m| m.contains("default")));
        let mut unknown = work();
        unknown.id = "account-9".into();
        assert!(refused(&mut settings, unknown).is_some());
        save(&mut settings, work()).unwrap_or_else(|e| panic!("{e:?}"));
        let twice = save(&mut settings, work()).err();
        assert_eq!(twice.map(|e| e.code), Some(ErrorCode::Conflict));
        // Renaming keeps the folder; moving it would lose its sessions.
        let mut renamed = work();
        renamed.id = "account-1".into();
        renamed.name = "Job".into();
        save(&mut settings, renamed.clone()).unwrap_or_else(|e| panic!("{e:?}"));
        renamed.config_dir = Some(crate::test_path!("/home/me/.claude-job").into());
        assert!(refused(&mut settings, renamed).is_some_and(|m| m.contains("cannot change")));
    }

    #[test]
    fn the_default_account_can_be_renamed_but_not_removed() {
        let mut settings = AgentSettings::default();
        let mut personal = ClaudeAccount::default_account();
        personal.name = "Personal".into();
        save(&mut settings, personal).unwrap_or_else(|e| panic!("{e:?}"));
        save(&mut settings, work()).unwrap_or_else(|e| panic!("{e:?}"));
        let names: Vec<_> = settings
            .all_accounts()
            .into_iter()
            .map(|a| a.name)
            .collect();
        assert_eq!(names, ["Personal", "Work"]);
        assert!(remove(&mut settings, DEFAULT_ACCOUNT).is_err());
        assert!(remove(&mut settings, "account-9").is_err());
    }

    #[test]
    fn a_session_runs_with_the_chosen_then_the_projects_then_the_default_account() {
        let mut settings = AgentSettings {
            claude_path: Some("/bin/claude".into()),
            ..AgentSettings::default()
        };
        save(&mut settings, work()).unwrap_or_else(|e| panic!("{e:?}"));
        let project = ProjectId("p1".into());
        let chosen = |s: &AgentSettings, explicit: Option<&str>| {
            choose(s, &project, explicit).unwrap_or_else(|e| panic!("{e:?}"))
        };
        assert_eq!(
            chosen(&settings, None),
            Chosen {
                id: None,
                executable: Some("/bin/claude".into()),
                config_dir: None
            }
        );
        let work = chosen(&settings, Some("account-1"));
        assert_eq!(work.id.as_deref(), Some("account-1"));
        assert_eq!(
            work.config_dir.as_deref(),
            Some(crate::test_path!("/home/me/.claude-work"))
        );
        assert!(choose(&settings, &project, Some("gone")).is_err());

        set_project(&mut settings, &project, Some("account-1".into()))
            .unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(chosen(&settings, None).id.as_deref(), Some("account-1"));
        assert_eq!(chosen(&settings, Some(DEFAULT_ACCOUNT)).id, None);
        assert!(set_project(&mut settings, &project, Some("gone".into())).is_err());

        // Removing the account sends the project back to the default.
        remove(&mut settings, "account-1").unwrap_or_else(|e| panic!("{e:?}"));
        assert!(settings.project_accounts.is_empty());
        assert_eq!(chosen(&settings, None).id, None);
    }

    #[test]
    fn settings_saved_before_accounts_still_load() {
        let old: AgentSettings = serde_json::from_str(r#"{"claude_path": "/opt/claude"}"#)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(old.claude_path.as_deref(), Some("/opt/claude"));
        assert_eq!(old.all_accounts(), [ClaudeAccount::default_account()]);
        let session: crate::protocol::agents::SessionInfo =
            serde_json::from_value(serde_json::json!({
                "id": "s1", "project": "p1", "agent": "claude", "mode": "direct",
                "status": {"state": "stopped"}, "title": "t",
                "usage": {"input_tokens": 0, "output_tokens": 0, "cost_micro_usd": null}
            }))
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(session.account, None);
    }
}
