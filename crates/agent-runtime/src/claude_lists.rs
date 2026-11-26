//! What `claude` keeps of its own in the home folder and needs from Ovra's
//! environment (ADR 0006, ADR 0011): the sandboxes give its state back and
//! the environment allowlists keep its variables. Data only: the sandbox
//! crates' tests compile this file as their own module, so their plan
//! snapshots are of these lists. Those tests include it with `#[path]`:
//! it must name nothing through `crate::` or `super::`, only `ovra_core`.

use ovra_core::services::ProviderLists;

pub(crate) const CLAUDE_LISTS: ProviderLists = ProviderLists {
    name: "Claude",
    home_dir: ".claude",
    state_file: ".claude.json",
    // Set only for an account the user added with its own folder.
    config_var: "CLAUDE_CONFIG_DIR",
    // Shell snapshots are sourced before each Bash call of the person's own
    // sessions; the rest is session data they read.
    private: &[
        "backups",
        "cache",
        "debug",
        "file-history",
        "ide",
        "logs",
        "paste-cache",
        "plans",
        "session-env",
        "sessions",
        "shell-snapshots",
        "statsig",
        "telemetry",
        "todos",
    ],
    // Not `shell-snapshots/` and `session-env/`: the person's own sessions
    // source them later.
    in_place: &[
        "backups",
        "cache",
        "debug",
        "file-history",
        "ide",
        "logs",
        "paste-cache",
        "plans",
        "sessions",
        "statsig",
        "todos",
    ],
    // Its login (the known limit in the spec) and its prompt history.
    files: &[".credentials.json", "history.jsonl"],
    files_made: &["history.jsonl"],
    projects: "projects",
    project_memory: "memory",
    // Whole names only: a prefix such as `CLAUDE_CODE_` would let
    // `CLAUDE_CODE_OAUTH_TOKEN` through.
    env: &[
        // Where usage is billed, and the gateway it goes through.
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_MODEL",
        "ANTHROPIC_SMALL_FAST_MODEL",
        // Cloud providers: which one, its region and project, never its keys.
        "CLAUDE_CODE_USE_BEDROCK",
        "CLAUDE_CODE_USE_VERTEX",
        "CLAUDE_CODE_USE_FOUNDRY",
        "ANTHROPIC_BEDROCK_BASE_URL",
        "ANTHROPIC_VERTEX_BASE_URL",
        "ANTHROPIC_VERTEX_PROJECT_ID",
        "ANTHROPIC_FOUNDRY_RESOURCE",
        "ANTHROPIC_FOUNDRY_BASE_URL",
        "AWS_REGION",
        "CLOUD_ML_REGION",
        "GOOGLE_CLOUD_PROJECT",
    ],
    // Where `claude` finds Git Bash.
    windows_env: &["CLAUDE_CODE_GIT_BASH_PATH"],
    fixture_var: "OVRA_FAKE_CLAUDE_FIXTURE",
};
