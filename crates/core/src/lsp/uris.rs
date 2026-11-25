//! URIs between the UI and a server (`docs/specs/lsp.md`, "URIs"). The UI
//! names files `ovra:///<path in the project>`; servers see `file://` URIs
//! under the root. Only the places LSP puts URIs are rewritten, never
//! document text.

use std::path::Path;

use serde_json::Value;

use crate::path::RelPath;
use crate::protocol::{ErrorCode, ProtocolError};

/// The UI's name for the project root; files are under it.
pub(crate) const ROOT: &str = "ovra:///";

/// Keys whose string value is a URI.
const KEYS: [&str; 7] = [
    "uri",
    "rootUri",
    "targetUri",
    "oldUri",
    "newUri",
    "scopeUri",
    "baseUri",
];

/// A `WorkspaceEdit`'s map from URIs to edits.
const MAP_KEY: &str = "changes";

/// Rewrites a client message for the server: `ovra:` URIs become `file:`
/// URIs under `root`, and `rootPath` becomes the root. A `file:` URI, or
/// an `ovra:` one that leaves the project, is refused.
pub(crate) fn to_server(message: &mut Value, root: &Path) -> Result<(), ProtocolError> {
    walk(message, &mut |key, value| match key {
        Key::Uri | Key::Changes => {
            if let Some(text) = value.as_str()
                && let Some(uri) = to_file(text, root)?
            {
                *value = Value::String(uri);
            }
            Ok(())
        }
        Key::RootPath => {
            *value = Value::String(root.to_string_lossy().into_owned());
            Ok(())
        }
    })
}

/// Rewrites a server message for the client: `file:` URIs under `root`
/// become `ovra:` URIs; others pass as they are.
pub(crate) fn to_client(message: &mut Value, root: &Path) {
    let root = slashed(root);
    let _ = walk(message, &mut |key, value| {
        if let (Key::Uri | Key::Changes, Some(text)) = (key, value.as_str())
            && let Some(uri) = to_ovra(text, &root)
        {
            *value = Value::String(uri);
        }
        Ok(())
    });
}

/// The `file:` URI of `root` itself.
pub(crate) fn file_uri(root: &Path) -> String {
    let path = slashed(root);
    let path = if path.starts_with('/') {
        path
    } else {
        format!("/{path}")
    };
    format!("file://{}", encode(&path))
}

#[derive(Clone, Copy)]
enum Key {
    Uri,
    Changes,
    RootPath,
}

/// Calls `f` for each value under a URI key, each key of a `changes` map
/// (as a string value it may replace), and each `rootPath`.
fn walk(
    value: &mut Value,
    f: &mut impl FnMut(Key, &mut Value) -> Result<(), ProtocolError>,
) -> Result<(), ProtocolError> {
    match value {
        Value::Array(items) => items.iter_mut().try_for_each(|v| walk(v, f)),
        Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                if KEYS.contains(&key.as_str()) && child.is_string() {
                    f(Key::Uri, child)?;
                } else if key == "rootPath" && child.is_string() {
                    f(Key::RootPath, child)?;
                } else if key == MAP_KEY {
                    if let Value::Object(changes) = child {
                        let mut renamed = serde_json::Map::new();
                        for (uri, edits) in std::mem::take(changes) {
                            let mut uri = Value::String(uri);
                            f(Key::Changes, &mut uri)?;
                            let uri = uri.as_str().unwrap_or_default().to_owned();
                            renamed.insert(uri, edits);
                        }
                        *changes = renamed;
                    }
                    walk(child, f)?;
                } else {
                    walk(child, f)?;
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// `Some(file URI)` for an `ovra:` URI, `None` for another scheme
/// (`untitled:`, `https:`), an error for `file:` or a path outside.
fn to_file(uri: &str, root: &Path) -> Result<Option<String>, ProtocolError> {
    if has_scheme(uri, "file") {
        return Err(bad(
            "the client names files as ovra:/// URIs, not file: ones",
        ));
    }
    let Some(rest) = uri.strip_prefix(ROOT) else {
        if has_scheme(uri, "ovra") {
            return Err(bad(format!("not a file of the project: {uri}")));
        }
        return Ok(None);
    };
    let rest = rest.split(['?', '#']).next().unwrap_or_default();
    let decoded = rest
        .split('/')
        .map(decode)
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| bad(format!("not a valid URI: {uri}")))?
        .join("/");
    let path = if decoded.is_empty() || decoded == "/" {
        RelPath::root()
    } else {
        RelPath::parse(decoded.trim_end_matches('/'))?
    };
    Ok(Some(file_uri(&path.under(root))))
}

/// The `ovra:` URI for a `file:` URI under `root` (with `/` separators).
fn to_ovra(uri: &str, root: &str) -> Option<String> {
    if !has_scheme(uri, "file") {
        return None;
    }
    let rest = &uri["file:".len()..];
    let rest = rest.strip_prefix("//").unwrap_or(rest);
    // `file://host/path`: only an empty host or `localhost` is this machine.
    let path = match rest.find('/') {
        Some(0) => rest,
        Some(i) if rest[..i].eq_ignore_ascii_case("localhost") => &rest[i..],
        _ => return None,
    };
    let path = path.split(['?', '#']).next().unwrap_or_default();
    let path = decode(path)?;
    let path = strip_root(&path, root)?;
    let encoded: Vec<String> = path.split('/').map(encode).collect();
    Some(format!("{ROOT}{}", encoded.join("/")))
}

/// `path` relative to `root`, both with `/` separators. Windows paths in
/// URIs start with `/C:/`, and drive letters compare in any case.
fn strip_root<'a>(path: &'a str, root: &str) -> Option<&'a str> {
    let root = root.trim_end_matches('/');
    let path_cmp = path
        .strip_prefix('/')
        .filter(|p| has_drive(p))
        .unwrap_or(path);
    let root_cmp = root
        .strip_prefix('/')
        .filter(|r| has_drive(r))
        .unwrap_or(root);
    let offset = path.len() - path_cmp.len();
    let same = |a: &str, b: &str| {
        if has_drive(a) && has_drive(b) {
            a[..1].eq_ignore_ascii_case(&b[..1]) && a[1..] == b[1..]
        } else {
            a == b
        }
    };
    let head = path_cmp.get(..root_cmp.len())?;
    if !same(head, root_cmp) {
        return None;
    }
    let rest = &path[offset + root_cmp.len()..];
    if rest.is_empty() {
        return Some("");
    }
    rest.strip_prefix('/')
}

fn has_drive(path: &str) -> bool {
    let b = path.as_bytes();
    b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}

fn has_scheme(uri: &str, scheme: &str) -> bool {
    uri.len() > scheme.len()
        && uri.as_bytes()[scheme.len()] == b':'
        && uri[..scheme.len()].eq_ignore_ascii_case(scheme)
}

/// `path` with `/` separators (Windows paths use `\`).
fn slashed(path: &Path) -> String {
    let text = path.to_string_lossy();
    if cfg!(windows) {
        text.replace('\\', "/")
    } else {
        text.into_owned()
    }
}

/// Percent-encodes everything but unreserved characters, `/`, and the
/// colon of a drive letter at the start (`/C:/...`).
fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (i, b) in text.bytes().enumerate() {
        let drive_colon = b == b':'
            && (i == 1 || (i == 2 && text.starts_with('/')))
            && text.as_bytes()[i - 1].is_ascii_alphabetic();
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) || drive_colon {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Percent-decodes `text`; `None` if it is not valid UTF-8 after decoding.
fn decode(text: &str) -> Option<String> {
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

fn bad(message: impl Into<String>) -> ProtocolError {
    ProtocolError::new(ErrorCode::BadRequest, message)
}
