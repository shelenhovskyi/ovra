//! Project allowlists for the netproxy (`docs/specs/security.md`): what a
//! host entry may be, which addresses are never reachable, and the
//! `netproxy.*` requests. The proxies themselves are `ovra-netproxy`.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use crate::Core;
use crate::protocol::netproxy::{
    AllowedHost, Allowlist, NetproxyEvent, NetproxyRequest, NetproxyResponse, ProjectAllowlist,
};
use crate::protocol::{ErrorCode, ProtocolError, Response};

/// More entries than anyone keeps by hand.
const MAX_HOSTS: usize = 256;

/// Hosts (and their subdomains) where anyone can receive data: code hosts,
/// file shares, cloud storage, paste sites, tunnels and app hosting.
const BROAD: [&str; 33] = [
    "github.com",
    "githubusercontent.com",
    "github.io",
    "gitlab.com",
    "bitbucket.org",
    "amazonaws.com",
    "cloudfront.net",
    "googleapis.com",
    "googleusercontent.com",
    "appspot.com",
    "web.app",
    "firebaseapp.com",
    "blob.core.windows.net",
    "azurewebsites.net",
    "r2.dev",
    "r2.cloudflarestorage.com",
    "workers.dev",
    "pages.dev",
    "vercel.app",
    "netlify.app",
    "herokuapp.com",
    "fly.dev",
    "ngrok.io",
    "ngrok-free.app",
    "trycloudflare.com",
    "pastebin.com",
    "gist.github.com",
    "dropbox.com",
    "drive.google.com",
    "docs.google.com",
    "discord.com",
    "slack.com",
    "webhook.site",
];

pub(crate) fn handle(core: &Core, request: NetproxyRequest) -> Result<Response, ProtocolError> {
    let store = core.project_store();
    let project = match request {
        NetproxyRequest::GetAllowlist { project } => {
            let allowlist = store.allowlist(&project)?;
            return Ok(answer(core, allowlist));
        }
        NetproxyRequest::SetAllowlist { project, allowlist } => {
            let allowlist = normalize(allowlist)?;
            core.sessions().set_allowlist(store, &project, &allowlist)?;
            core.emit(NetproxyEvent::Changed {
                project: project.clone(),
            });
            project
        }
    };
    Ok(answer(core, store.allowlist(&project)?))
}

fn answer(core: &Core, project: Allowlist) -> Response {
    let broad = project
        .hosts
        .iter()
        .filter(|h| is_broad(&h.host))
        .map(|h| h.host.clone())
        .collect();
    Response::Netproxy(NetproxyResponse::Allowlist(ProjectAllowlist {
        base: core.sessions().net_base(),
        project,
        broad,
    }))
}

/// Checks and normalizes every entry, dropping repeats.
fn normalize(allowlist: Allowlist) -> Result<Allowlist, ProtocolError> {
    let bad = |message: String| ProtocolError::new(ErrorCode::BadRequest, message);
    if allowlist.hosts.len() > MAX_HOSTS {
        return Err(bad(format!("at most {MAX_HOSTS} hosts")));
    }
    let mut hosts: Vec<AllowedHost> = Vec::with_capacity(allowlist.hosts.len());
    for entry in allowlist.hosts {
        let host = parse_host(&entry.host).map_err(|why| bad(format!("{}: {why}", entry.host)))?;
        if entry.port == Some(0) {
            return Err(bad(format!("{host}: port 0 is not a port")));
        }
        let entry = AllowedHost {
            host,
            port: entry.port,
        };
        if !hosts.contains(&entry) {
            hosts.push(entry);
        }
    }
    Ok(Allowlist { hosts })
}

/// A host as the allowlist keeps it: a lowercase DNS name without a
/// trailing dot, or an IP address in its usual form. Refuses wildcards,
/// anything else that is not a plain name, and addresses that are never
/// reachable ([`is_reachable`]).
pub(crate) fn parse_host(raw: &str) -> Result<String, &'static str> {
    let host = raw.trim().to_ascii_lowercase();
    let host = host.strip_suffix('.').unwrap_or(&host);
    let unbracketed = host
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(host);
    if let Ok(ip) = unbracketed.parse::<IpAddr>() {
        if !is_reachable(ip) {
            return Err("loopback, private and link-local addresses are never reachable");
        }
        return Ok(ip.to_string());
    }
    if host.is_empty() {
        return Err("the host is empty");
    }
    if host.contains('*') {
        return Err("wildcards are not supported; list each host");
    }
    if host.len() > 253 {
        return Err("the name is too long");
    }
    let labels: Vec<&str> = host.split('.').collect();
    let label_ok = |l: &&str| {
        !l.is_empty()
            && l.len() <= 63
            && !l.starts_with('-')
            && !l.ends_with('-')
            && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    };
    if !labels.iter().all(label_ok) {
        return Err("not a host name: use letters, digits, dots and dashes only");
    }
    // `127.1` or `0x7f.1` are addresses to some resolvers, not names.
    if labels
        .last()
        .is_some_and(|l| l.bytes().all(|b| b.is_ascii_digit()) || l.starts_with("0x"))
    {
        return Err("not a host name or a plain address");
    }
    Ok(host.to_owned())
}

/// Whether `host` (as [`parse_host`] keeps it) is one where anyone can
/// receive data, so that allowing it lets an agent send the project out.
pub(crate) fn is_broad(host: &str) -> bool {
    BROAD
        .iter()
        .any(|b| host == *b || host.strip_suffix(b).is_some_and(|rest| rest.ends_with('.')))
}

/// Whether a session may connect to `ip` at all: only public unicast
/// addresses. Loopback, private, link-local (the metadata service),
/// shared, documentation, multicast and reserved ranges are refused
/// whatever name led to them.
pub fn is_reachable(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4_reachable(v4),
        IpAddr::V6(v6) => v6_reachable(v6),
    }
}

fn v4_reachable(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(a == 0
        || a == 10
        || a == 127
        || a >= 224
        || (a == 100 && (64..128).contains(&b))
        || (a == 169 && b == 254)
        || (a == 172 && (16..32).contains(&b))
        || (a == 192 && b == 168)
        || (a == 192 && b == 0 && (c == 0 || c == 2))
        || (a == 192 && b == 88 && c == 99)
        || (a == 198 && (b == 18 || b == 19))
        || (a == 198 && b == 51 && c == 100)
        || (a == 203 && b == 0 && c == 113))
}

fn v6_reachable(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return v4_reachable(v4);
    }
    let s = ip.segments();
    // Only global unicast, 2000::/3.
    if s[0] & 0xe000 != 0x2000 {
        return false;
    }
    match s[0] {
        // Protocol assignments (Teredo, benchmarking, ...) and documentation.
        0x2001 if s[1] < 0x0200 || s[1] == 0x0db8 => false,
        // 6to4 carries an IPv4 address.
        0x2002 => v4_reachable(Ipv4Addr::new(
            (s[1] >> 8) as u8,
            s[1] as u8,
            (s[2] >> 8) as u8,
            s[2] as u8,
        )),
        // Documentation, 3fff::/20.
        0x3fff if s[1] < 0x1000 => false,
        _ => true,
    }
}

#[cfg(test)]
mod tests;
