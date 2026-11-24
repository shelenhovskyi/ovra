//! Orchestration and review state (`docs/specs/orchestration.md`): tasks,
//! roles, limits and review items. Stub until tasks 3.B and 3.D.

use crate::protocol::orchestration::OrchestrationRequest;
use crate::protocol::review::ReviewRequest;
use crate::protocol::{ErrorCode, ProtocolError, Response};

fn not_yet(what: &str, task: &str) -> ProtocolError {
    ProtocolError::new(
        ErrorCode::Unavailable,
        format!("{what} is not implemented yet (task {task})"),
    )
}

pub(crate) fn review(_: ReviewRequest) -> Result<Response, ProtocolError> {
    Err(not_yet("review", "3.B"))
}

pub(crate) fn orchestration(_: OrchestrationRequest) -> Result<Response, ProtocolError> {
    Err(not_yet("orchestration", "3.B"))
}

#[cfg(test)]
mod tests {
    use crate::Core;
    use crate::protocol::{ErrorCode, Request};

    #[tokio::test]
    async fn review_and_orchestration_say_they_are_not_ready() {
        let core = Core::new("test");
        for json in [
            serde_json::json!({"method": "review.list", "params": {"project": "p1"}}),
            serde_json::json!({"method": "orchestration.list", "params": {"project": "p1"}}),
        ] {
            let request: Request = serde_json::from_value(json).unwrap_or_else(|e| panic!("{e}"));
            let code = core.handle(request).await.err().map(|e| e.code);
            assert_eq!(code, Some(ErrorCode::Unavailable));
        }
    }
}
