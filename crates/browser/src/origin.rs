//! The origin of a URL, as the person is asked about it: `scheme://host`
//! with the port only when it is not the scheme's default. It follows
//! what browsers do with a URL (WHATWG), not a split at `://`, so a URL
//! cannot show one origin and load another:
//! `http://localhost:5173@evil.example/` is `http://evil.example`.

/// The origin of an `http` or `https` URL; `None` for any other URL and
/// for one whose host is not a plain name or an IP address.
pub(crate) fn origin(url: &str) -> Option<String> {
    // Browsers drop leading and trailing C0 controls and spaces, and tabs
    // and newlines anywhere.
    let url: String = url
        .trim_matches(|c: char| c <= ' ')
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect();
    let (scheme, rest) = url.split_once(':')?;
    let scheme = scheme.to_ascii_lowercase();
    let default = match scheme.as_str() {
        "http" => 80,
        "https" => 443,
        _ => return None,
    };
    // For these schemes `\` is `/`, and any number of slashes may follow.
    let rest = rest.trim_start_matches(['/', '\\']);
    let authority = &rest[..rest.find(['/', '\\', '?', '#']).unwrap_or(rest.len())];
    // Whatever is before the last `@` is a user name and password.
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let (host, port) = match host_port.rfind(':') {
        Some(at) if !host_port[at..].contains(']') => (&host_port[..at], &host_port[at + 1..]),
        _ => (host_port, ""),
    };
    let host = host.to_ascii_lowercase();
    let name = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_');
    let ip6 = |h: &str| {
        h.strip_prefix('[')
            .and_then(|h| h.strip_suffix(']'))
            .is_some_and(|h| {
                !h.is_empty() && h.chars().all(|c| c.is_ascii_hexdigit() || ":.".contains(c))
            })
    };
    if host.is_empty() || !(host.chars().all(name) || ip6(&host)) {
        return None;
    }
    if port.is_empty() {
        return Some(format!("{scheme}://{host}"));
    }
    if !port.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    match port.parse::<u16>().ok()? {
        n if n == default => Some(format!("{scheme}://{host}")),
        n => Some(format!("{scheme}://{host}:{n}")),
    }
}

#[cfg(test)]
mod tests {
    use super::origin;

    #[test]
    fn origins_are_what_the_browser_loads() {
        let cases = [
            ("http://localhost:5173/", Some("http://localhost:5173")),
            (
                "http://localhost:5173@evil.example/",
                Some("http://evil.example"),
            ),
            (
                "https://user:pw@Example.COM:443/x?y#z",
                Some("https://example.com"),
            ),
            ("HTTP://example.com:80", Some("http://example.com")),
            ("http://example.com:0080/", Some("http://example.com")),
            ("https://example.com:8443", Some("https://example.com:8443")),
            ("  http:\\\\a.example\\b", Some("http://a.example")),
            ("http://a.exa\tmple/", Some("http://a.example")),
            ("http://[::1]:8080/", Some("http://[::1]:8080")),
            ("http://[::1]/", Some("http://[::1]")),
            ("http://127.0.0.1:3000?q", Some("http://127.0.0.1:3000")),
            (
                "http://example.com#@evil.example",
                Some("http://example.com"),
            ),
            ("http://example.com:99999/", None),
            ("http://example.com:8x/", None),
            ("http://ex%61mple.com/", None),
            ("http:///", None),
            ("about:blank", None),
            ("file:///etc/passwd", None),
            ("javascript:alert(1)", None),
            ("data:text/html,hi", None),
            ("localhost:5173", None),
        ];
        let got: Vec<_> = cases.iter().map(|(url, _)| origin(url)).collect();
        let want: Vec<_> = cases.iter().map(|(_, o)| o.map(str::to_owned)).collect();
        assert_eq!(got, want);
    }
}
