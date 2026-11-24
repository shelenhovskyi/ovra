//! The user's own proxy where Ovra runs (`HTTPS_PROXY`, `HTTP_PROXY`,
//! `NO_PROXY`), which the netproxy chains to.

use std::net::IpAddr;

use crate::url::{Authority, base64};

/// An `http://` proxy server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Server {
    pub(crate) host: String,
    pub(crate) port: u16,
    /// The `Proxy-Authorization` value for its URL's user and password.
    pub(crate) auth: Option<String>,
}

impl Server {
    fn parse(url: &str) -> Option<Self> {
        // Without a scheme, curl and Claude Code read it as http.
        let url = if url.contains("://") {
            url.to_owned()
        } else {
            format!("http://{url}")
        };
        let url = Authority::parse(&url).filter(|a| a.scheme == "http")?;
        Some(Self {
            host: url.host,
            port: url.port.unwrap_or(80),
            auth: url
                .userinfo
                .map(|u| format!("Basic {}", base64(u.as_bytes()))),
        })
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Upstream {
    /// For CONNECT tunnels.
    pub(crate) tunnel: Option<Server>,
    /// For plain HTTP requests.
    pub(crate) http: Option<Server>,
    no_proxy: Vec<String>,
}

impl Upstream {
    /// Read as Claude Code reads it: `https_proxy`, `HTTPS_PROXY`,
    /// `http_proxy`, `HTTP_PROXY` for tunnels. Proxies that are not
    /// `http://` (SOCKS, TLS to the proxy) are not supported and ignored.
    pub(crate) fn from_env(var: &dyn Fn(&str) -> Option<String>) -> Self {
        let first = |names: &[&str]| {
            names
                .iter()
                .find_map(|n| var(n).filter(|v| !v.trim().is_empty()))
                .and_then(|v| Server::parse(&v))
        };
        let no_proxy = var("no_proxy")
            .filter(|v| !v.trim().is_empty())
            .or_else(|| var("NO_PROXY"))
            .unwrap_or_default();
        Self {
            tunnel: first(&["https_proxy", "HTTPS_PROXY", "http_proxy", "HTTP_PROXY"]),
            http: first(&["http_proxy", "HTTP_PROXY"]),
            no_proxy: no_proxy
                .split([',', ' ', '\t'])
                .map(|e| e.trim().to_ascii_lowercase())
                .filter(|e| !e.is_empty())
                .collect(),
        }
    }

    /// The proxy for `host` (normalized), or `None` to connect directly.
    pub(crate) fn server(&self, host: &str, tunnel: bool) -> Option<&Server> {
        let server = if tunnel { &self.tunnel } else { &self.http };
        server.as_ref().filter(|_| !self.bypassed(host))
    }

    fn bypassed(&self, host: &str) -> bool {
        let ip = host.parse::<IpAddr>().ok();
        self.no_proxy.iter().any(|entry| {
            if entry == "*" {
                return true;
            }
            if let Some((net, bits)) = entry.split_once('/') {
                return match (ip, net.parse::<IpAddr>(), bits.parse::<u8>()) {
                    (Some(ip), Ok(net), Ok(bits)) => in_net(ip, net, bits),
                    _ => false,
                };
            }
            let name = entry
                .strip_prefix("*.")
                .or_else(|| entry.strip_prefix('.'))
                .unwrap_or(entry);
            let name = crate::url::host_port(name).map_or_else(|| name.to_owned(), |(h, _)| h);
            host == name
                || host
                    .strip_suffix(name.as_str())
                    .is_some_and(|rest| rest.ends_with('.'))
        })
    }
}

fn in_net(ip: IpAddr, net: IpAddr, bits: u8) -> bool {
    match (ip, net) {
        (IpAddr::V4(ip), IpAddr::V4(net)) if bits <= 32 => {
            let mask = u32::MAX.checked_shl(32 - u32::from(bits)).unwrap_or(0);
            u32::from(ip) & mask == u32::from(net) & mask
        }
        (IpAddr::V6(ip), IpAddr::V6(net)) if bits <= 128 => {
            let mask = u128::MAX.checked_shl(128 - u32::from(bits)).unwrap_or(0);
            u128::from(ip) & mask == u128::from(net) & mask
        }
        _ => false,
    }
}
