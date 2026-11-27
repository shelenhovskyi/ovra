//! `UseKey` rules: whether the netproxy may add a key to a session's
//! request without asking (`docs/specs/secrets-consent.md`). They decide
//! as `Push` rules do: `deny`, then `ask`, then `auto`, which counts only
//! in a sandboxed session with a limited network.

use crate::protocol::agents::{SandboxNetwork, SandboxReport};
use crate::protocol::policy::{Policy, PolicyAction, PolicyRule};

use super::text;

/// The pseudo-tool of rules for keys: `UseKey(<key name>)`.
pub(crate) const KEY_TOOL: &str = "UseKey";

/// Why a `UseKey` rule is refused, if it is.
pub(crate) fn refused(rule: &PolicyRule) -> Option<&'static str> {
    if rule.exact {
        return Some("a UseKey rule matches a key's name, never a command");
    }
    match &rule.pattern {
        Some(name) if !crate::secrets::name_ok(name) => {
            Some("a UseKey match is a key's name, such as github")
        }
        // Every key at once would be too wide to skip the question for.
        None if rule.action == PolicyAction::Auto => Some("an auto UseKey rule names one key"),
        _ => None,
    }
}

/// What the project's `UseKey` rules say about key `name`.
pub(crate) fn decide(
    policy: &Policy,
    name: &str,
    sandbox: Option<&SandboxReport>,
) -> Option<(PolicyAction, String)> {
    let matching = |action: PolicyAction| {
        policy
            .rules
            .iter()
            .filter(|r| r.action == action && r.tool == KEY_TOOL)
            .find(|r| match &r.pattern {
                Some(pattern) => pattern == name,
                // A stored auto rule without a match allows nothing.
                None => action != PolicyAction::Auto,
            })
            .map(|r| (action, text(r)))
    };
    if let Some(decided) = matching(PolicyAction::Deny).or_else(|| matching(PolicyAction::Ask)) {
        return Some(decided);
    }
    contained(sandbox)
        .then(|| matching(PolicyAction::Auto))
        .flatten()
}

/// The rule "Always allow" offers for key `name`; none where `auto` never
/// counts.
pub(crate) fn always(name: &str, sandbox: Option<&SandboxReport>) -> Option<PolicyRule> {
    let rule = PolicyRule {
        tool: KEY_TOOL.to_owned(),
        pattern: Some(name.to_owned()),
        action: PolicyAction::Auto,
        exact: false,
        from_approval: true,
    };
    (contained(sandbox) && refused(&rule).is_none()).then_some(rule)
}

fn contained(sandbox: Option<&SandboxReport>) -> bool {
    sandbox.is_some_and(|s| s.network != SandboxNetwork::Open)
}
