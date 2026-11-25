//! `pr.*`: pull requests of reviewed workers (`docs/specs/pr-flow.md`).
//! Only `pr.status` works yet; the rest comes with PR.C.

use crate::Core;
use crate::protocol::pr::{PrRequest, PrResponse};
use crate::protocol::{ErrorCode, ProtocolError, Response};

/// The remote a request names none: the one `git clone` sets up.
const DEFAULT_REMOTE: &str = "origin";

pub(crate) async fn handle(core: &Core, request: PrRequest) -> Result<Response, ProtocolError> {
    match request {
        PrRequest::Status { project, remote } => {
            let root = core.root(&project)?;
            let remote = remote_name(remote)?;
            let status = core.forge().status(root, remote).await?;
            Ok(Response::Pr(PrResponse::Status(status)))
        }
        PrRequest::Prepare { .. }
        | PrRequest::Create { .. }
        | PrRequest::Update { .. }
        | PrRequest::Refresh { .. }
        | PrRequest::SendLog { .. } => Err(ProtocolError::new(
            ErrorCode::Unavailable,
            "pull requests are not available yet",
        )),
    }
}

/// A remote's name as git allows it on a command line: never a URL, a
/// path or something `git` or `gh` would read as an option.
fn remote_name(remote: Option<String>) -> Result<String, ProtocolError> {
    let remote = remote.unwrap_or_else(|| DEFAULT_REMOTE.to_owned());
    let plain = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.');
    if remote.is_empty() || remote.starts_with(['-', '.']) || !remote.chars().all(plain) {
        return Err(ProtocolError::new(
            ErrorCode::BadRequest,
            format!("{remote:?} is not a remote's name"),
        ));
    }
    Ok(remote)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;

    use crate::Core;
    use crate::fakes::{MemoryForge, MemoryProjects};
    use crate::protocol::pr::PrResponse;
    use crate::protocol::{ErrorCode, Request, Response};

    async fn call(core: &Core, json: serde_json::Value) -> Result<Response, ErrorCode> {
        let request: Request = serde_json::from_value(json).unwrap_or_else(|e| panic!("{e}"));
        core.handle(request).await.map_err(|e| e.code)
    }

    #[tokio::test]
    async fn status_asks_the_forge_and_the_rest_is_not_available_yet() {
        let forge = Arc::new(MemoryForge::default());
        let core = Core::builder("test")
            .projects(Arc::new(MemoryProjects::default()))
            .forge(forge.clone())
            .build();
        let root = crate::test_path!("/work/a");
        let add = json!({"method": "projects.add", "params": {"root": root}});
        assert!(call(&core, add).await.is_ok());

        let status = json!({"method": "pr.status", "params": {"project": "p1"}});
        let Ok(Response::Pr(PrResponse::Status(gh))) = call(&core, status).await else {
            panic!("no status");
        };
        assert!(gh.signed_in);
        assert_eq!(
            *forge.asked.lock().unwrap_or_else(|e| e.into_inner()),
            [(root.into(), "origin".to_owned())]
        );
        for remote in ["https://x@github.com/o/r", "--upload-pack=x", "", "../r"] {
            let asked =
                json!({"method": "pr.status", "params": {"project": "p1", "remote": remote}});
            assert_eq!(call(&core, asked).await.err(), Some(ErrorCode::BadRequest));
        }
        let prepare = json!({"method": "pr.prepare", "params": {"id": "r1"}});
        assert_eq!(
            call(&core, prepare).await.err(),
            Some(ErrorCode::Unavailable)
        );
    }
}
