//! Which origins a session's browser may load. A granted server of the
//! session (`http://localhost:P` or `http://127.0.0.1:P` while its grant
//! holds) loads at once; any other origin asks the person once per session
//! and waits for `browser.answer_navigation`, refused when nobody answers
//! in time. Every answer is kept for the session and audited.

use std::time::Duration;

use tokio::sync::watch;

use crate::Core;
use crate::protocol::agents::SessionId;
use crate::protocol::audit::{AuditEvent, NavigationBy};
use crate::protocol::browser::{BrowserEvent, PageId};
use crate::protocol::{ErrorCode, ProtocolError};

use super::{Result, emit_pages, page_not_found};

/// How long a navigation waits for the person (the browser itself waits
/// longer).
pub(super) const ASK_TIMEOUT: Duration = Duration::from_secs(120);

/// Whether the session's browser may load `origin` (`scheme://host[:port]`).
pub(super) async fn decide(
    core: &Core,
    session: &SessionId,
    page: &PageId,
    origin: String,
) -> bool {
    if origin == "about:blank" || granted(core, session, &origin) {
        return true;
    }
    let (mut rx, first) = {
        let mut st = core.browser_state().lock();
        let Some(pages) = st.sessions.get_mut(session) else {
            return false;
        };
        // A page may ask before its `open` answered.
        pages.had.insert(page.clone());
        if let Some(answer) = pages.answers.get(&origin) {
            return *answer;
        }
        match pages.asks.get(&origin) {
            Some(tx) => (tx.subscribe(), false),
            None => {
                let (tx, rx) = watch::channel(None);
                pages.asks.insert(origin.clone(), tx);
                (rx, true)
            }
        }
    };
    if first {
        core.emit(BrowserEvent::NavigationAsked {
            page: page.clone(),
            session: session.clone(),
            origin: origin.clone(),
        });
    }
    let waited = tokio::time::timeout(ASK_TIMEOUT, rx.wait_for(Option::is_some)).await;
    match waited {
        Ok(Ok(answer)) => answer.unwrap_or(false),
        // The session ended: its asks went with it.
        Ok(Err(_)) => false,
        Err(_) => {
            let gave_up = first && {
                let mut st = core.browser_state().lock();
                let pages = st.sessions.get_mut(session);
                pages.is_some_and(|p| p.asks.remove(&origin).is_some())
            };
            if gave_up {
                audit(core, session, origin, false, NavigationBy::NoAnswer);
            }
            false
        }
    }
}

/// The person's answer for the session of `page`, kept for the session.
pub(super) fn answer(core: &Core, page: &PageId, origin: String, allow: bool) -> Result<()> {
    let session = {
        let mut st = core.browser_state().lock();
        let (session, pages) = st
            .sessions
            .iter_mut()
            .find(|(_, p)| p.had.contains(page))
            .ok_or_else(|| page_not_found(page))?;
        let ask = pages.asks.remove(&origin).ok_or_else(|| {
            ProtocolError::new(
                ErrorCode::NotFound,
                format!("no navigation to {origin} waits"),
            )
        })?;
        pages.answers.insert(origin.clone(), allow);
        ask.send_replace(Some(allow));
        session.clone()
    };
    audit(core, &session, origin, allow, NavigationBy::Person);
    Ok(())
}

/// Ends the session's browser and refuses what its pages wait for.
pub(crate) fn session_ended(core: &Core, session: &SessionId) {
    let state = core.browser_state();
    let ended = state.lock().sessions.remove(session);
    state.browser.end(session);
    let Some(pages) = ended else {
        return;
    };
    for origin in pages.asks.keys() {
        audit(
            core,
            session,
            origin.clone(),
            false,
            NavigationBy::SessionEnded,
        );
    }
    if !pages.open.is_empty() {
        let closed = super::Pages::default();
        emit_pages(core, session, &closed);
    }
}

/// Whether `origin` is exactly `http://localhost:P` or `http://127.0.0.1:P`
/// for a port the session's netproxy is granted now.
fn granted(core: &Core, session: &SessionId, origin: &str) -> bool {
    let Some((host, port)) = origin
        .strip_prefix("http://")
        .and_then(|rest| rest.rsplit_once(':'))
    else {
        return false;
    };
    let loopback = host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1";
    let port = Some(port)
        .filter(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|p| p.parse::<u16>().ok());
    match port {
        Some(port) if loopback => crate::servers::granted_ports(core, session).contains(&port),
        _ => false,
    }
}

fn audit(core: &Core, session: &SessionId, origin: String, allowed: bool, by: NavigationBy) {
    let Ok(project) = core.sessions().find(session, |s| s.info().project) else {
        return;
    };
    let event = AuditEvent::BrowserNavigation {
        origin,
        allowed,
        by,
    };
    crate::audit::record(core, &project, Some(session), event);
}
