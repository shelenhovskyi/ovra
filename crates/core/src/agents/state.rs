//! The session state machine. Runtimes report states as they see them; the
//! core keeps only changes that make sense, so the UI never shows a session
//! leaving `stopped`, or `idle` while a permission request waits.

use crate::protocol::agents::AgentStatus;

/// Whether a session may go from `from` to `to`. Pending permission requests
/// are checked by the caller: while any waits, only `waiting_for_approval`
/// and the end states apply.
pub(crate) fn allowed(from: &AgentStatus, to: &AgentStatus) -> bool {
    use AgentStatus::{Failed, Idle, Starting, Stopped, WaitingForApproval, Working};
    match (from, to) {
        (Stopped | Failed { .. }, _) => false,
        (_, Stopped | Failed { .. }) => true,
        (Starting, Working | WaitingForApproval | Idle)
        | (Working, WaitingForApproval | Idle)
        | (WaitingForApproval, Working)
        | (Idle, Working | WaitingForApproval) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transitions() {
        use AgentStatus::{Idle, Starting, Stopped, WaitingForApproval, Working};
        let failed = AgentStatus::Failed {
            message: "x".into(),
        };
        let all = [
            Starting,
            Working,
            WaitingForApproval,
            Idle,
            Stopped,
            failed.clone(),
        ];
        let table: Vec<String> = all
            .iter()
            .map(|from| {
                let to: Vec<String> = all
                    .iter()
                    .filter(|to| allowed(from, to))
                    .map(|to| format!("{to:?}"))
                    .collect();
                let to = if to.is_empty() {
                    "none".to_owned()
                } else {
                    to.join(", ")
                };
                format!("{from:?} -> {to}")
            })
            .collect();
        insta::assert_snapshot!(table.join("\n"), @r#"
        Starting -> Working, WaitingForApproval, Idle, Stopped, Failed { message: "x" }
        Working -> WaitingForApproval, Idle, Stopped, Failed { message: "x" }
        WaitingForApproval -> Working, Stopped, Failed { message: "x" }
        Idle -> Working, WaitingForApproval, Stopped, Failed { message: "x" }
        Stopped -> none
        Failed { message: "x" } -> none
        "#);
    }
}
