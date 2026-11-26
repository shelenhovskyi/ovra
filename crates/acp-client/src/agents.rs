//! The ACP agents Ovra knows, as data (`docs/specs/more-agents.md`): what
//! each one runs, what it keeps in the home folder, the variables it needs
//! and how it says it is signed out. The netproxy holds their hosts
//! (`ovra-netproxy`, `base.rs`) and the core the hosts that never take a
//! secret.

use ovra_core::protocol::agents::{AgentKind, Capabilities, ProviderInfo};
use ovra_core::services::ProviderLists;

use crate::AcpAgent;

/// What every ACP agent can do in v1 (`docs/specs/acp-adapter.md`).
const ACP: Capabilities = Capabilities {
    approvals: true,
    enforced_policy: false,
    ide_tools: false,
    read_only_role: false,
    resume: false,
    fork: false,
    interrupt: true,
    text_deltas: true,
    subagents: false,
    usage: false,
    cost: false,
    billing: false,
    accounts: false,
};

/// How a known agent is checked and how it says it needs a sign-in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Known {
    /// Arguments that make it print its version, or at least exit 0.
    pub check: &'static [&'static str],
    /// Words its `session/new` error has when it has no sign-in.
    pub signed_out: &'static [&'static str],
    /// What the person does to sign in, in the vendor's own words.
    pub sign_in: &'static str,
}

/// What `gemini` keeps in `~/.gemini`. Its settings stay read-only; its
/// chats and checkpoints (`tmp/`, `history/`, one folder per project) are
/// private to the session.
pub(crate) const GEMINI_LISTS: ProviderLists = ProviderLists {
    name: "Gemini",
    home_dir: ".gemini",
    // It has no state file next to its folder, and no accounts, so no
    // variable for an account's folder is ever set.
    state_file: ".gemini.json",
    config_var: "GEMINI_CLI_HOME",
    // The names it gives project folders, which it renames into place
    // under a lock file next to them: a session's own. So are the OAuth
    // tokens of the person's MCP servers (the file store, and the
    // encrypted one newer versions keep when there is no keychain).
    private: &[
        "tmp",
        "history",
        "projects.json",
        "mcp-oauth-tokens.json",
        "mcp-oauth-tokens-v2.json",
    ],
    in_place: &["tmp", "history"],
    // Its Google login, refreshed during a session.
    files: &["oauth_creds.json", "google_accounts.json"],
    layered: true,
    files_made: &[],
    projects: "tmp",
    project_memory: "memory",
    // Whole names only: where usage is billed and the gateway or cloud
    // project it goes through.
    env: &[
        "GEMINI_API_KEY",
        "GOOGLE_API_KEY",
        "GEMINI_MODEL",
        "GOOGLE_GEMINI_BASE_URL",
        "GOOGLE_GENAI_USE_VERTEXAI",
        "GOOGLE_GENAI_USE_GCA",
        "GOOGLE_CLOUD_PROJECT",
        "GOOGLE_CLOUD_LOCATION",
    ],
    windows_env: &[],
    fixture_var: "OVRA_FAKE_ACP_FIXTURE",
};

/// What `codex-acp` keeps in `~/.codex` (`CODEX_HOME`). Its settings stay
/// read-only; its sessions, logs, databases and history are private to the
/// session;
/// its login, which it rewrites, is writable in place.
pub(crate) const CODEX_LISTS: ProviderLists = ProviderLists {
    name: "Codex",
    home_dir: ".codex",
    // No state file next to its folder; `CODEX_HOME` is set only for an
    // account, which Codex has none of yet.
    state_file: ".codex.json",
    config_var: "CODEX_HOME",
    // Its databases too: a session's own, so it never shares the person's
    // with a write-ahead log of its own (which would corrupt them). Its
    // prompt history is one file for every project, and the OAuth tokens
    // of the person's MCP servers (where there is no keychain): a
    // session's own, so it reads neither.
    private: &[
        "sessions",
        "history.jsonl",
        ".credentials.json",
        "log",
        "tmp",
        "shell_snapshots",
        "state_5.sqlite",
        "state_5.sqlite-wal",
        "state_5.sqlite-shm",
        "logs_2.sqlite",
        "logs_2.sqlite-wal",
        "logs_2.sqlite-shm",
    ],
    in_place: &["sessions", "log", "tmp"],
    files: &["auth.json"],
    layered: true,
    files_made: &[],
    projects: "sessions",
    project_memory: "memory",
    env: &["OPENAI_API_KEY", "CODEX_API_KEY", "OPENAI_BASE_URL"],
    windows_env: &[],
    fixture_var: "OVRA_FAKE_ACP_FIXTURE",
};

pub(crate) const GEMINI: Known = Known {
    check: &["--version"],
    signed_out: &[
        "API key is missing",
        "Please set an Auth method",
        "must authenticate",
    ],
    sign_in: "Run gemini in a terminal once and sign in with Google, or set GEMINI_API_KEY.",
};

pub(crate) const CODEX: Known = Known {
    // It has no `--version`.
    check: &["--help"],
    signed_out: &["Authentication required"],
    sign_in: "Run codex login in a terminal, or set OPENAI_API_KEY.",
};

/// Gemini CLI, which speaks ACP itself.
pub fn gemini() -> AcpAgent {
    AcpAgent {
        info: ProviderInfo {
            id: AgentKind::Gemini,
            name: "Gemini".to_owned(),
            capabilities: ACP,
        },
        program: "gemini".into(),
        args: vec!["--acp".into()],
        lists: &GEMINI_LISTS,
        known: Some(GEMINI),
    }
}

/// Codex through Zed's `codex-acp`, which runs Codex itself.
pub fn codex() -> AcpAgent {
    AcpAgent {
        info: ProviderInfo {
            id: AgentKind::Codex,
            name: "Codex".to_owned(),
            capabilities: ACP,
        },
        program: "codex-acp".into(),
        args: Vec::new(),
        lists: &CODEX_LISTS,
        known: Some(CODEX),
    }
}

/// Every ACP agent Ovra knows, Gemini first.
pub fn known() -> Vec<AcpAgent> {
    vec![gemini(), codex()]
}

/// The vendor's sign-in hint when `error`, the agent's answer to
/// `session/new`, says it has none.
pub(crate) fn signed_out(known: &Known, error: &str) -> Option<&'static str> {
    let error = error.to_ascii_lowercase();
    known
        .signed_out
        .iter()
        .any(|w| error.contains(&w.to_ascii_lowercase()))
        .then_some(known.sign_in)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_known_agents_are_described() {
        let described: Vec<_> = known()
            .into_iter()
            .map(|a| (a.info, a.program, a.args, a.lists, a.known))
            .collect();
        insta::assert_debug_snapshot!(described);
    }

    #[test]
    fn every_agents_own_state_is_protected_in_every_sandbox() {
        use ovra_core::protected::AGENT_STATE_IN_HOME;
        for agent in known() {
            let lists = agent.lists;
            assert!(
                AGENT_STATE_IN_HOME.contains(&lists.home_dir),
                "{}",
                lists.name
            );
            assert!(
                AGENT_STATE_IN_HOME.contains(&lists.state_file),
                "{}",
                lists.name
            );
            assert!(lists.in_place.iter().all(|d| lists.private.contains(d)));
            assert!(lists.files_made.iter().all(|f| lists.files.contains(f)));
            assert!(lists.files.iter().all(|f| !lists.private.contains(f)));
        }
    }

    #[test]
    fn a_session_without_a_sign_in_gets_the_vendors_hint() {
        let gemini = "the agent did not start the session: Gemini API key is missing or not \
                      configured.";
        assert_eq!(signed_out(&GEMINI, gemini), Some(GEMINI.sign_in));
        let codex = "the agent did not start the session: Authentication required";
        assert_eq!(signed_out(&CODEX, codex), Some(CODEX.sign_in));
        assert_eq!(signed_out(&CODEX, "Codex exited with exit status: 3"), None);
        assert_eq!(signed_out(&GEMINI, codex), None);
    }
}
