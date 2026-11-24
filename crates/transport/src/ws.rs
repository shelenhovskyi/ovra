//! WebSocket endpoint for headless mode.
//!
//! Security (ADR 0003): the server is meant to bind to 127.0.0.1 only, every
//! connection must present the per-launch token, and browsers must send an
//! `Origin` from the allowed list.

use std::sync::Arc;

use axum::Router;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use futures_util::{SinkExt, StreamExt};
use ovra_core::Core;
use ovra_core::protocol::info::InfoEvent;
use serde::Deserialize;
use tokio::sync::broadcast::error::RecvError;

use crate::{dispatch, event_frame};

/// Who may connect.
#[derive(Debug, Clone)]
pub struct Access {
    /// The per-launch secret clients pass as `?token=`.
    pub token: String,
    /// Exact `Origin` values accepted, e.g. `http://127.0.0.1:7777`.
    /// Requests without an `Origin` header (non-browser clients) are accepted
    /// as long as the token matches.
    pub allowed_origins: Vec<String>,
}

#[derive(Debug, Clone)]
struct AppState {
    core: Core,
    access: Arc<Access>,
}

#[derive(Debug, Deserialize)]
struct WsQuery {
    token: Option<String>,
}

/// Builds a router serving the WebSocket endpoint at `/ws`.
pub fn router(core: Core, access: Access) -> Router {
    Router::new()
        .route("/ws", get(upgrade))
        .with_state(AppState {
            core,
            access: Arc::new(access),
        })
}

async fn upgrade(
    State(state): State<AppState>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    if !token_matches(query.token.as_deref(), &state.access.token) {
        return (StatusCode::UNAUTHORIZED, "bad token").into_response();
    }
    if let Some(origin) = headers.get(header::ORIGIN) {
        let allowed = origin
            .to_str()
            .is_ok_and(|o| state.access.allowed_origins.iter().any(|a| a == o));
        if !allowed {
            return (StatusCode::FORBIDDEN, "origin not allowed").into_response();
        }
    }
    ws.on_upgrade(move |socket| serve(socket, state.core))
}

/// Compares in constant time so the token cannot be guessed byte by byte.
fn token_matches(given: Option<&str>, expected: &str) -> bool {
    let Some(given) = given else { return false };
    if given.len() != expected.len() {
        return false;
    }
    given
        .bytes()
        .zip(expected.bytes())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

async fn serve(socket: WebSocket, core: Core) {
    let (mut tx, mut rx) = socket.split();
    let mut events = core.subscribe();
    if tx
        .send(Message::Text(event_frame(InfoEvent::Ready.into()).into()))
        .await
        .is_err()
    {
        return;
    }
    loop {
        tokio::select! {
            incoming = rx.next() => match incoming {
                Some(Ok(Message::Text(text))) => {
                    let reply = dispatch(&core, &text).await;
                    if tx.send(Message::Text(reply.into())).await.is_err() {
                        break;
                    }
                }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                Some(Ok(_)) => {}
            },
            event = events.recv() => match event {
                Ok(event) => {
                    if tx.send(Message::Text(event_frame(event).into())).await.is_err() {
                        break;
                    }
                }
                Err(RecvError::Lagged(skipped)) => {
                    tracing::warn!(skipped, "client lagged behind core events");
                }
                Err(RecvError::Closed) => break,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_comparison() {
        assert!(token_matches(Some("abc"), "abc"));
        assert!(!token_matches(Some("abd"), "abc"));
        assert!(!token_matches(Some("ab"), "abc"));
        assert!(!token_matches(None, "abc"));
    }
}
