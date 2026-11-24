//! The little URL handling the proxy needs: `scheme://[user@]host[:port]/...`
//! and `host:port`.

/// Where a URL points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Authority {
    pub(crate) scheme: String,
    /// `user:password`, percent-decoded.
    pub(crate) userinfo: Option<String>,
    /// Lowercase, without brackets or a trailing dot.
    pub(crate) host: String,
    pub(crate) port: Option<u16>,
}

impl Authority {
    /// An `http` or `https` URL; `None` for anything else.
    pub(crate) fn parse(url: &str) -> Option<Self> {
        let (scheme, rest) = url.trim().split_once("://")?;
        let scheme = scheme.to_ascii_lowercase();
        if scheme != "http" && scheme != "https" {
            return None;
        }
        let authority = rest.split(['/', '?', '#']).next()?;
        let (userinfo, hostport) = match authority.rsplit_once('@') {
            Some((user, hostport)) => (Some(percent_decode(user)?), hostport),
            None => (None, authority),
        };
        let (host, port) = host_port(hostport)?;
        Some(Self {
            scheme,
            userinfo,
            host,
            port,
        })
    }
}

/// `host`, `host:port`, `[v6]` or `[v6]:port`: the host normalized and the
/// port if there is one. `None` for an empty host or a bad port.
pub(crate) fn host_port(text: &str) -> Option<(String, Option<u16>)> {
    let (host, port) = if let Some(rest) = text.strip_prefix('[') {
        let (host, after) = rest.split_once(']')?;
        match after {
            "" => (host, None),
            _ => (host, Some(after.strip_prefix(':')?)),
        }
    } else {
        match text.rsplit_once(':') {
            Some((host, port)) if !host.contains(':') => (host, Some(port)),
            Some(_) => return None,
            None => (text, None),
        }
    };
    let port = match port {
        Some(port) => Some(port.parse::<u16>().ok().filter(|p| *p != 0)?),
        None => None,
    };
    let host = host.to_ascii_lowercase();
    let host = host.strip_suffix('.').unwrap_or(&host);
    if host.is_empty() {
        return None;
    }
    // Addresses in their usual form, so `::0:1` and `::1` are one host.
    let host = match host.parse::<std::net::IpAddr>() {
        Ok(ip) => ip.to_string(),
        Err(_) => host.to_owned(),
    };
    Some((host, port))
}

fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = text.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Standard base64, for `Proxy-Authorization: Basic`.
pub(crate) fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[((n >> shift) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}
