//! A tool call's input as the audit log keeps it (`docs/specs/security.md`):
//! file tools by path and the SHA-256 of what they write, everything else as
//! JSON with the values of the project's secrets, token-like strings,
//! credential fields and the values of `Authorization` and `Cookie` headers
//! removed, cut to 4 KiB.

use std::fmt::Write as _;

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::protocol::agents::ToolKind;
use crate::protocol::audit::AuditInput;
use crate::protocol::secrets::SecretValue;
use crate::services::ToolRequest;

/// The most bytes of JSON (or of a path) an entry keeps.
pub(crate) const MAX_INPUT_BYTES: usize = 4096;

pub(crate) const REDACTED: &str = "[redacted]";

/// Claude's own tools, whose names are kept even when the secrets' values
/// cannot be read.
const BUILT_IN_TOOLS: [&str; 18] = [
    "Bash",
    "BashOutput",
    "Edit",
    "ExitPlanMode",
    "Glob",
    "Grep",
    "KillShell",
    "LS",
    "MultiEdit",
    "NotebookEdit",
    "NotebookRead",
    "Read",
    "SlashCommand",
    "Task",
    "TodoWrite",
    "WebFetch",
    "WebSearch",
    "Write",
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

/// Common header names: one after a secret header's name and a comma is the
/// next name in a list, not its value.
const HEADER_NAMES: [&str; 16] = [
    "accept",
    "accept-encoding",
    "accept-language",
    "authorization",
    "cache-control",
    "content-length",
    "content-type",
    "cookie",
    "host",
    "origin",
    "proxy-authorization",
    "referer",
    "set-cookie",
    "user-agent",
    "x-api-key",
    "x-requested-with",
];

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
///
/// An `edit` call of one file is kept as that file and the hash of what it
/// writes (`ToolRequest::written`); every other call as its input.
pub(crate) fn input(tool: &ToolRequest, known: Option<&[SecretValue]>) -> AuditInput {
    let known = known.map(Known::new);
    let call = &tool.call;
    if call.kind == ToolKind::Edit && call.paths.len() <= 1 {
        let path = call.paths.first().map_or("", String::as_str);
        return AuditInput::File {
            path: match &known {
                Some(known) => cut(redact_string(path, known)),
                None => REDACTED.to_owned(),
            },
            sha256: tool.written.as_deref().map(|text| sha256(text.as_bytes())),
        };
    }
    let Some(known) = known else {
        return AuditInput::Json {
            text: REDACTED.to_owned(),
        };
    };
    let mut kept = tool.input.clone();
    redact(&mut kept, &known);
    AuditInput::Json {
        text: cut(kept.to_string()),
    }
}

/// A tool name as kept: agents choose them, so they are redacted and cut
/// like any text. Without the secrets' values only the names of Claude's
/// own tools are kept: any other (an MCP tool's) could hold one.
pub(crate) fn cut_name(tool: &str, known: Option<&[SecretValue]>) -> String {
    match known {
        Some(known) => cut(redact_string(tool, &Known::new(known))),
        None if BUILT_IN_TOOLS.contains(&tool) => tool.to_owned(),
        None => REDACTED.to_owned(),
    }
}

/// A string without the known values, header values and token-like words,
/// cut to what the entry could show.
fn redact_string(text: &str, known: &Known) -> String {
    redact_text(clip(&redact_known(text, known)))
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

fn redact(value: &mut Value, known: &Known) {
    match value {
        Value::String(text) => *text = redact_string(text, known),
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
                    (redact_string(&name, known), field)
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

/// The project's secret values in every form looked for: as they are,
/// JSON-escaped (a JSON body inside a command) and percent-encoded (a URL
/// or form body), in upper- and lower-case hex.
pub(crate) struct Known {
    forms: Vec<String>,
    longest: usize,
}

impl Known {
    pub(crate) fn new(values: &[SecretValue]) -> Self {
        let mut forms: Vec<String> = Vec::new();
        for value in values.iter().map(SecretValue::expose) {
            if value.is_empty() {
                continue;
            }
            let json = serde_json::to_string(value).unwrap_or_default();
            let json = json[1..json.len().max(2) - 1].to_owned();
            let slashes = json.replace('/', "\\/");
            for form in [
                value.to_owned(),
                json,
                slashes,
                percent(value, true),
                percent(value, false),
            ] {
                if !form.is_empty() && !forms.contains(&form) {
                    forms.push(form);
                }
            }
        }
        let longest = forms.iter().map(String::len).max().unwrap_or(0);
        Self { forms, longest }
    }
}

/// `value` with every byte but `A-Z a-z 0-9 - . _ ~` as `%XX`.
fn percent(value: &str, upper: bool) -> String {
    value.bytes().fold(String::new(), |mut out, b| {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(char::from(b));
        } else if upper {
            let _ = write!(out, "%{b:02X}");
        } else {
            let _ = write!(out, "%{b:02x}");
        }
        out
    })
}

/// The largest char boundary of `text` at or before `at`.
fn floor(text: &str, at: usize) -> usize {
    let mut at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// `text` without any of the `known` values, and without what [`clip`]
/// would drop anyway.
///
/// Only the part a value starting before the clip can reach is searched,
/// and overlapping places merge as they are found, so a short value in a
/// huge input costs no more than in a small one. A value that starts
/// before the clip goes whole; text past it is dropped, back to a word
/// boundary, so no value cut there shows its start.
fn redact_known(text: &str, known: &Known) -> String {
    let limit = floor(text, 2 * MAX_INPUT_BYTES);
    let scan = &text[..floor(text, limit + known.longest)];
    let mut covered: Vec<(usize, usize)> = Vec::new();
    for value in &known.forms {
        let mut run: Option<(usize, usize)> = None;
        let mut from = 0;
        while let Some(at) = scan.get(from..).and_then(|rest| rest.find(value.as_str())) {
            let start = from + at;
            if start >= limit {
                break;
            }
            let end = start + value.len();
            match &mut run {
                Some((_, run_end)) if start <= *run_end => *run_end = (*run_end).max(end),
                _ => covered.extend(run.replace((start, end))),
            }
            // The next char, so a value that overlaps itself is found again.
            from = start + text[start..].chars().next().map_or(1, char::len_utf8);
        }
        covered.extend(run);
    }
    covered.sort_unstable();
    let mut out = String::with_capacity(limit.min(text.len()));
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
    if copied < limit {
        let rest = &text[copied..limit];
        if limit < text.len() {
            // Cut: drop the word cut in half, as `clip` does.
            let keep = rest.len() - rest.bytes().rev().take_while(|b| token_byte(*b)).count();
            out.push_str(&rest[..keep]);
        } else {
            out.push_str(rest);
        }
    }
    out
}

/// Whether `text` holds a token-like word, a secret header's value or one
/// of the `known` values (the `@` context's check, `crate::context`).
pub(crate) fn holds_secret(text: &str, known: &[SecretValue]) -> bool {
    redact_text(text) != text
        || known
            .iter()
            .map(SecretValue::expose)
            .any(|value| !value.is_empty() && text.contains(value))
}

/// Text an agent wrote, as the log keeps it: without header values and
/// token-like words, at most [`MAX_INPUT_BYTES`].
pub(crate) fn redacted_text(text: &str) -> String {
    cut(redact_text(clip(text)))
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
/// name, `,` (`set("Cookie", ...)`), unless what follows the comma is
/// another header name. The value may start on the next line (YAML).
/// Inside a quoted value an escaped quote does not close it.
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
        let mut listed = false;
        let quoted = quote(&mut i).is_some();
        // `h["Cookie"] = `, and Ruby's `h[:authorization] = `.
        if bytes.get(i) == Some(&b']') {
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
            Some(b',') if quoted => {
                listed = true;
                i += 1;
            }
            _ => {
                at = start + len;
                continue;
            }
        }
        blank(&mut i);
        // A YAML value on the next line, maybe after a block indicator
        // (`|`, `>-`, ...): one line break and the indentation go.
        let mut next = i;
        if matches!(bytes.get(next), Some(b'|' | b'>')) {
            next += 1;
            while matches!(bytes.get(next), Some(b'-' | b'+' | b'0'..=b'9')) {
                next += 1;
            }
            blank(&mut next);
        }
        if bytes.get(next) == Some(&b'\r') {
            next += 1;
        }
        if bytes.get(next) == Some(&b'\n') {
            i = next + 1;
            blank(&mut i);
        }
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
        // The next name in a list of names (`["authorization", "accept"]`).
        let name = listed && HEADER_NAMES.contains(&&lower[value..end]);
        if end > value && !name {
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
