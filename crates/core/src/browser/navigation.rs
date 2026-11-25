//! Which origins a session's browser may load: a granted server of the
//! session (`http://localhost:P` or `http://127.0.0.1:P` while its grant
//! holds) and `about:blank`; any other origin is refused.

use crate::Core;
use crate::protocol::agents::SessionId;
use crate::protocol::browser::PageId;

use super::emit_pages;

/// Whether the session's browser may load `origin` (`scheme://host[:port]`).
pub(super) async fn decide(core: &Core, session: &SessionId, _: &PageId, origin: String) -> bool {
    origin == "about:blank" || granted(core, session, &origin)
}

/// Ends the session's browser.
pub(crate) fn session_ended(core: &Core, session: &SessionId) {
    let state = core.browser_state();
    let ended = state.lock().sessions.remove(session);
    state.browser.end(session);
    let Some(pages) = ended else {
        return;
    };
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
