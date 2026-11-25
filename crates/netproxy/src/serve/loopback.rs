//! The loopback rule (`docs/specs/servers-and-browser.md`, "The netproxy
//! rule"): exactly `localhost:P` or `127.0.0.1:P` reaches the pod of a
//! managed server the core granted on `P`, through the grant's dial, never
//! a host port. Plain HTTP only, one request per connection; `Host` must
//! name the target and `Origin`, if any, a granted server. No secret is
//! ever added here.

use std::collections::BTreeMap;
use std::io;

use ovra_core::services::LoopbackGrant;
use tokio::time::timeout;

use super::{DIAL_TIMEOUT, Request, SessionNet, one_request, respond};
use crate::dial::Stream;

/// One session's grants by port.
pub(crate) type Grants = BTreeMap<u16, LoopbackGrant>;

/// The grants of one `NetProxy::grant` call; a later one for the same port
/// wins.
pub(crate) fn grants(list: Vec<LoopbackGrant>) -> Grants {
    list.into_iter().map(|g| (g.port, g)).collect()
}

/// The port of exactly `localhost[:P]` (any case) or `127.0.0.1[:P]`, 80
/// without one. Nothing else counts: no trailing dot, no other spelling.
pub(super) fn authority(text: &str) -> Option<u16> {
    let (host, port) = match text.split_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (text, None),
    };
    if !host.eq_ignore_ascii_case("localhost") && host != "127.0.0.1" {
        return None;
    }
    match port {
        None => Some(80),
        Some(p) if (1..=5).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_digit()) => {
            p.parse::<u16>().ok().filter(|p| *p != 0)
        }
        Some(_) => None,
    }
}

/// The rest of an `OVRA-LOCAL <P>\r\n` line: the port and what follows.
pub(super) fn preamble(after: &[u8]) -> Option<(u16, &[u8])> {
    let end = after.windows(2).position(|w| w == b"\r\n")?;
    let digits = std::str::from_utf8(&after[..end]).ok()?;
    if !(1..=5).contains(&digits.len()) || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let port = digits.parse::<u16>().ok().filter(|p| *p != 0)?;
    Some((port, &after[end + 2..]))
}

/// Checks the request against the rule: the grant to dial and the host
/// name to send on, or the refusal's reason.
fn check(request: &Request, session: &SessionNet) -> Result<(LoopbackGrant, String), &'static str> {
    if request.connect {
        return Err("connect to loopback");
    }
    let grants = session.grants.borrow();
    let grant = grants.get(&request.port).ok_or("no grant")?.clone();
    let host = match request.hosts.as_slice() {
        [one] => std::str::from_utf8(one)
            .ok()
            .filter(|h| authority(h) == Some(request.port)),
        _ => None,
    }
    .ok_or("bad host")?;
    let origin_ok = match request.origins.as_slice() {
        [] => true,
        [one] => std::str::from_utf8(one)
            .ok()
            .and_then(|o| o.strip_prefix("http://"))
            .and_then(authority)
            .is_some_and(|port| grants.contains_key(&port)),
        _ => false,
    };
    if !origin_ok {
        return Err("bad origin");
    }
    let name = host.split_once(':').map_or(host, |(name, _)| name);
    Ok((grant, name.to_ascii_lowercase()))
}

pub(super) async fn serve(
    client: &mut impl Stream,
    mut request: Request,
    rest: Vec<u8>,
    session: &SessionNet,
) -> io::Result<()> {
    let name = request.name();
    let (grant, host) = match check(&request, session) {
        Ok(checked) => checked,
        Err(why) => {
            session.reporter.refused(&name, why);
            return respond(client, 403, why).await;
        }
    };
    request.host = host;
    let server = match timeout(DIAL_TIMEOUT, (grant.dial)()).await {
        Ok(Ok(server)) => server,
        Ok(Err(e)) => {
            let why = format!("could not reach the server: {}", e.message);
            return respond(client, 502, &why).await;
        }
        Err(_) => return respond(client, 502, "reaching the server timed out").await,
    };
    let head = request.head(None);
    one_request(client, server, &head, &rest, request.body).await
}

#[cfg(test)]
mod tests;
