//! Which addresses phone access may listen on: this computer's private
//! ones (RFC 1918, `100.64.0.0/10` for Tailscale, IPv6 unique-local),
//! never a public, loopback or unspecified address.

use std::net::{IpAddr, SocketAddr, TcpListener};

pub(crate) fn is_private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            a == 10
                || (a == 172 && (16..32).contains(&b))
                || (a == 192 && b == 168)
                || (a == 100 && (64..128).contains(&b))
        }
        IpAddr::V6(v6) => (v6.segments()[0] & 0xfe00) == 0xfc00,
    }
}

/// This computer's private addresses, IPv4 first; with `loopback` (tests)
/// 127.0.0.1 too.
pub(crate) fn private_addresses(loopback: bool) -> Vec<IpAddr> {
    let mut found: Vec<IpAddr> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .map(|i| i.ip())
        .filter(|ip| is_private(*ip))
        .collect();
    if loopback {
        found.push(IpAddr::from([127, 0, 0, 1]));
    }
    found.sort_by_key(|ip| (ip.is_ipv6(), ip.to_string()));
    found.dedup();
    found
}

/// Binds `ip` on the port used before, else on a free port from the
/// dynamic range.
pub(crate) fn bind(ip: IpAddr, port: Option<u16>) -> std::io::Result<TcpListener> {
    if let Some(port) = port
        && let Ok(listener) = TcpListener::bind(SocketAddr::new(ip, port))
    {
        return Ok(listener);
    }
    let mut last = None;
    for _ in 0..20 {
        let port = 49152 + rand::random::<u16>() % (65535 - 49152);
        match TcpListener::bind(SocketAddr::new(ip, port)) {
            Ok(listener) => return Ok(listener),
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("no free port")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_private_addresses_count() {
        for ip in [
            "192.168.1.2",
            "10.0.0.5",
            "172.20.1.1",
            "100.100.1.1",
            "fd00::1",
        ] {
            assert!(is_private(ip.parse().expect("ip")), "{ip}");
        }
        for ip in [
            "8.8.8.8",
            "127.0.0.1",
            "0.0.0.0",
            "172.32.0.1",
            "100.128.0.1",
            "2001:db8::1",
            "::1",
            "fe80::1",
        ] {
            assert!(!is_private(ip.parse().expect("ip")), "{ip}");
        }
    }
}
