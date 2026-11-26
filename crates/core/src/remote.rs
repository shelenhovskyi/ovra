//! `remote.*`: the desktop's side of phone access (ADR 0014). The service
//! does the work; requests from a phone never reach here
//! (`companion::allows`).

use crate::Core;
use crate::protocol::remote::{RemoteRequest, RemoteResponse};
use crate::protocol::{OkResponse, ProtocolError, Response};

pub(crate) fn handle(core: &Core, request: RemoteRequest) -> Result<Response, ProtocolError> {
    let remote = core.remote();
    Ok(match request {
        RemoteRequest::Status => Response::Remote(RemoteResponse::Status(remote.status()?)),
        RemoteRequest::Enable { address } => Response::Remote(RemoteResponse::Status(
            remote.enable(core.clone(), address)?,
        )),
        RemoteRequest::Disable => Response::Remote(RemoteResponse::Status(remote.disable()?)),
        RemoteRequest::StartPairing => {
            Response::Remote(RemoteResponse::Pairing(remote.start_pairing()?))
        }
        RemoteRequest::AnswerPairing { id, allow } => {
            remote.answer_pairing(&id, allow)?;
            Response::Ok(OkResponse::Ok)
        }
        RemoteRequest::Devices => Response::Remote(RemoteResponse::Devices(remote.devices()?)),
        RemoteRequest::Forget { device } => {
            remote.forget(&device)?;
            Response::Ok(OkResponse::Ok)
        }
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;

    use crate::Core;
    use crate::fakes::FakeRemote;
    use crate::protocol::remote::RemoteResponse;
    use crate::protocol::{ErrorCode, Request, Response};

    async fn call(core: &Core, value: serde_json::Value) -> Result<Response, ErrorCode> {
        let request: Request = serde_json::from_value(value).unwrap_or_else(|e| panic!("{e}"));
        core.handle(request).await.map_err(|e| e.code)
    }

    #[tokio::test]
    async fn the_desktop_turns_phone_access_on_pairs_and_forgets() {
        let remote = Arc::new(FakeRemote::default());
        let core = Core::builder("test").remote(remote.clone()).build();
        assert_eq!(
            call(&core, json!({"method": "remote.start_pairing"}))
                .await
                .err(),
            Some(ErrorCode::Conflict)
        );
        match call(&core, json!({"method": "remote.enable", "params": {}})).await {
            Ok(Response::Remote(RemoteResponse::Status(s))) => assert!(s.enabled),
            other => panic!("unexpected {other:?}"),
        }
        match call(&core, json!({"method": "remote.start_pairing"})).await {
            Ok(Response::Remote(RemoteResponse::Pairing(p))) => assert!(p.url.contains("/pair#")),
            other => panic!("unexpected {other:?}"),
        }
        call(
            &core,
            json!({"method": "remote.answer_pairing", "params": {"id": "q1", "allow": true}}),
        )
        .await
        .unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(
            *remote.answers.lock().expect("lock"),
            [("q1".to_owned(), true)]
        );
        assert_eq!(
            call(
                &core,
                json!({"method": "remote.forget", "params": {"device": "nope"}})
            )
            .await
            .err(),
            Some(ErrorCode::NotFound)
        );
    }

    #[tokio::test]
    async fn a_phone_cannot_reach_phone_access_settings() {
        let core = Core::builder("test")
            .remote(Arc::new(FakeRemote::default()))
            .build();
        let phone = core.connect_phone("Pixel");
        for method in [
            "remote.status",
            "remote.enable",
            "remote.start_pairing",
            "remote.devices",
        ] {
            let request: Request = serde_json::from_value(if method == "remote.enable" {
                json!({"method": method, "params": {}})
            } else {
                json!({"method": method})
            })
            .unwrap_or_else(|e| panic!("{e}"));
            assert_eq!(
                phone.handle(request).await.err().map(|e| e.code),
                Some(ErrorCode::Forbidden)
            );
        }
    }
}
