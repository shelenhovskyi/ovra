//! A tool call's input as the audit log keeps it (`docs/specs/security.md`):
//! file tools by path and the SHA-256 of what they write, everything else as
//! JSON with the values of the project's secrets, token-like strings,
//! credential fields and the values of `Authorization` and `Cookie` headers
//! removed, cut to 4 KiB.

use std::fmt::Write as _;

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::protocol::audit::AuditInput;
use crate::protocol::secrets::SecretValue;

/// The most bytes of JSON (or of a path) an entry keeps.
pub(crate) const MAX_INPUT_BYTES: usize = 4096;

pub(crate) const REDACTED: &str = "[redacted]";

/// File tools: their path field and the field with what they write.
const FILE_TOOLS: [(&str, &str, &str); 4] = [
    ("Write", "file_path", "content"),
    ("Edit", "file_path", "new_string"),
    ("MultiEdit", "file_path", "edits"),
    ("NotebookEdit", "notebook_path", "new_source"),
];

/// How known tokens start; each needs at least [`MIN_TOKEN_TAIL`] more
/// characters to count. JWTs are recognised by their shape instead.
const TOKEN_PREFIXES: [&str; 11] = [
    "sk-",
    "ghp_",
    "gho_",
    "ghu_",
    "ghs_",
    "ghr_",
    "github_pat_",
    "glpat-",
    "xox",
    "AKIA",
    "ASIA",
];
const MIN_TOKEN_TAIL: usize = 8;

/// Header names whose value is cut out of any text, compared in lower case
/// (`cookie` also covers `Set-Cookie`, `authorization` also
/// `Proxy-Authorization`), when a separator follows them (see
/// [`redact_headers`]).
const SECRET_HEADERS: [&str; 2] = ["authorization", "cookie"];

/// Field names whose value is never kept, whatever it is: compared in
/// lower case without `-` and `_`, by their end (`accessToken`,
/// `x-api-key`, `client_secret`, ...).
const SECRET_FIELD_ENDS: [&str; 10] = [
    "authorization",
    "token",
    "secret",
    "password",
    "passwd",
    "apikey",
    "privatekey",
    "accesskey",
    "secretkey",
    "credentials",
];

/// `known` holds the values of the project's secrets; `None` when they
/// could not be read, and then nothing the agent wrote is kept but a hash.
pub(crate) fn input(tool: &str, input: &Value, known: Option<&[SecretValue]>) -> AuditInput {
    if let Some((_, path, content)) = FILE_TOOLS.iter().find(|(name, ..)| *name == tool) {
        let path = input.get(*path).and_then(Value::as_str).unwrap_or("");
        return AuditInput::File {
            path: match known {
                Some(known) => cut(redact_text(clip(&redact_known(path, known)))),
                None => REDACTED.to_owned(),
            },
            sha256: input.get(*content).map(|content| match content {
                Value::String(text) => sha256(text.as_bytes()),
                other => sha256(other.to_string().as_bytes()),
            }),
        };
    }
    let Some(known) = known else {
        return AuditInput::Json {
            text: REDACTED.to_owned(),
        };
    };
    let mut kept = input.clone();
    redact(&mut kept, known);
    AuditInput::Json {
        text: cut(kept.to_string()),
    }
}

/// A tool name as kept: agents choose them, so they are redacted and cut
/// like any text.
pub(crate) fn cut_name(tool: &str, known: Option<&[SecretValue]>) -> String {
    cut(redact_text(clip(&redact_known(tool, known.unwrap_or(&[])))))
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

fn redact(value: &mut Value, known: &[SecretValue]) {
    match value {
        Value::String(text) => *text = redact_text(clip(&redact_known(text, known))),
        Value::Array(items) => items.iter_mut().for_each(|item| redact(item, known)),
        Value::Object(fields) => {
            *fields = std::mem::take(fields)
                .into_iter()
                .map(|(name, mut field)| {
                    if secret_field(&name) {
                        field = Value::String(REDACTED.to_owned());
                    } else {
                        redact(&mut field, known);
                    }
                    (redact_text(clip(&redact_known(&name, known))), field)
                })
                .collect();
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn secret_field(name: &str) -> bool {
    let name: String = name
        .chars()
        .filter(|c| *c != '-' && *c != '_')
        .flat_map(char::to_lowercase)
        .collect();
    name == "cookie"
        || name == "setcookie"
        || SECRET_FIELD_ENDS.iter().any(|end| name.ends_with(end))
}

/// `text` without any of the `known` values. Every place one occurs is
/// marked first, so values that overlap each other are removed whole.
fn redact_known(text: &str, known: &[SecretValue]) -> String {
    let mut covered: Vec<(usize, usize)> = Vec::new();
    for value in known.iter().map(SecretValue::expose) {
        if value.is_empty() {
            continue;
        }
        let mut from = 0;
        while let Some(at) = text[from..].find(value) {
            let start = from + at;
            covered.push((start, start + value.len()));
            // The next char, so a value that overlaps itself is found again.
            from = start + text[start..].chars().next().map_or(1, char::len_utf8);
        }
    }
    if covered.is_empty() {
        return text.to_owned();
    }
    covered.sort_unstable();
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut covered = covered.into_iter().peekable();
    while let Some((start, mut end)) = covered.next() {
        while let Some(&(next, next_end)) = covered.peek() {
            if next > end {
                break;
            }
            end = end.max(next_end);
            covered.next();
        }
        out.push_str(&text[copied..start]);
        out.push_str(REDACTED);
        copied = end;
    }
    out.push_str(&text[copied..]);
    out
}

/// `text` without header values and token-like words.
pub(crate) fn redact_text(text: &str) -> String {
    redact_tokens(&redact_headers(text))
}

/// Cuts the value after each secret header name: a quoted value up to its
/// closing quote, any other up to the end of the line or a quote.
///
/// The name may be quoted, also with escaped quotes (`\"Cookie\"`, as in a
/// JSON body inside a shell command), and closed by `]` (`h["Cookie"] = `).
/// It is separated from its value by `:`, `=`, `=>` or, after a quoted
/// name, `,` (`set("Cookie", ...)`). Inside a quoted value an escaped quote
/// does not close it.
fn redact_headers(text: &str) -> String {
    // ASCII lower-casing keeps every byte where it was.
    let lower = text.to_ascii_lowercase();
    let bytes = text.as_bytes();
    let blank = |i: &mut usize| {
        while matches!(bytes.get(*i), Some(b' ' | b'\t')) {
            *i += 1;
        }
    };
    // An opening or closing quote at `i`, maybe escaped: the quote and
    // whether it was.
    let quote = |i: &mut usize| {
        let escaped = bytes.get(*i) == Some(&b'\\');
        let at = *i + usize::from(escaped);
        let quote = *bytes.get(at).filter(|b| matches!(b, b'"' | b'\''))?;
        *i = at + 1;
        Some((quote, escaped))
    };
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut at = 0;
    while let Some((start, len)) = SECRET_HEADERS
        .iter()
        .filter_map(|name| Some((lower[at..].find(name)? + at, name.len())))
        .min()
    {
        let mut i = start + len;
        let quoted = quote(&mut i).is_some();
        if quoted && bytes.get(i) == Some(&b']') {
            i += 1;
        }
        blank(&mut i);
        match bytes.get(i) {
            Some(b':') => i += 1,
            Some(b'=') => {
                i += if bytes.get(i + 1) == Some(&b'>') {
                    2
                } else {
                    1
                }
            }
            Some(b',') if quoted => i += 1,
            _ => {
                at = start + len;
                continue;
            }
        }
        blank(&mut i);
        let (value, end) = match quote(&mut i) {
            Some((quote, escaped)) => {
                let value = i;
                // The first quote that is not escaped, or, in a value that
                // opened with an escaped one, the first quote at all.
                let end = (value..bytes.len())
                    .find(|&n| bytes[n] == quote && (escaped || bytes[n - 1] != b'\\'))
                    .map_or(bytes.len(), |n| {
                        if escaped && bytes[n - 1] == b'\\' {
                            n - 1
                        } else {
                            n
                        }
                    });
                (value, end.max(value))
            }
            None => {
                let end = bytes[i..]
                    .iter()
                    .position(|b| matches!(b, b'"' | b'\'' | b'\r' | b'\n'))
                    .map_or(bytes.len(), |n| i + n);
                (i, end)
            }
        };
        if end > value {
            out.push_str(&text[copied..value]);
            out.push_str(REDACTED);
            copied = end;
        }
        at = end.max(start + len);
    }
    out.push_str(&text[copied..]);
    out
}

fn word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

fn token_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"-_.+/=".contains(&b)
}

/// Replaces each word that looks like a token: one of [`TOKEN_PREFIXES`]
/// with a long enough tail, or a JWT. A word starts a run of token bytes,
/// or follows one of its bytes that cannot be part of a word (`.`, `/`, ...),
/// and always ends where the run does.
fn redact_tokens(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut start = 0;
    while start < bytes.len() {
        if !token_byte(bytes[start]) {
            start += 1;
            continue;
        }
        let end = start
            + bytes[start..]
                .iter()
                .take_while(|b| token_byte(**b))
                .count();
        let found = (start..end)
            .find(|&at| (at == 0 || !word_byte(bytes[at - 1])) && looks_like_token(&text[at..end]));
        if let Some(at) = found {
            out.push_str(&text[copied..at]);
            out.push_str(REDACTED);
            copied = end;
        }
        start = end;
    }
    out.push_str(&text[copied..]);
    out
}

fn looks_like_token(word: &str) -> bool {
    if word.starts_with("eyJ") {
        return word.split('.').filter(|part| !part.is_empty()).count() >= 3;
    }
    TOKEN_PREFIXES
        .iter()
        .any(|prefix| word.starts_with(prefix) && word.len() >= prefix.len() + MIN_TOKEN_TAIL)
}

/// The start of a string that the cut JSON could show, without a word cut
/// in half there (its short tail would not look like a token any more).
fn clip(text: &str) -> &str {
    if text.len() <= 2 * MAX_INPUT_BYTES {
        return text;
    }
    let mut end = 2 * MAX_INPUT_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let bytes = text.as_bytes();
    while end > 0 && token_byte(bytes[end - 1]) {
        end -= 1;
    }
    &text[..end]
}

/// At most [`MAX_INPUT_BYTES`], ending in `…` when cut.
fn cut(mut text: String) -> String {
    if text.len() <= MAX_INPUT_BYTES {
        return text;
    }
    let mut end = MAX_INPUT_BYTES - '…'.len_utf8();
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text.push('…');
    text
}
