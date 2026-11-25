//! `secrets.*` (`docs/specs/secrets.md`): the core checks every field, so
//! each `Secrets` implementation stores only what the netproxy can send
//! safely, then runs the keychain call on a blocking thread.

use crate::Core;
use crate::protocol::projects::ProjectId;
use crate::protocol::secrets::{
    SecretInfo, SecretList, SecretValue, SecretsEvent, SecretsRequest, SecretsResponse,
};
use crate::protocol::{ErrorCode, OkResponse, ProtocolError, Response};

const MAX_NAME: usize = 64;
const MAX_HEADER: usize = 64;
const MAX_PREFIX: usize = 64;
const MAX_VALUE: usize = 16 * 1024;

/// Domains `claude` itself talks to; the proxy must never decrypt them
/// (ADR 0006). Their subdomains are refused too.
const CLAUDE_DOMAINS: [&str; 3] = ["anthropic.com", "claude.ai", "claude.com"];

/// Headers that frame or route a request; a secret never replaces them.
/// Every `Proxy-*` header is refused as well.
const REFUSED_HEADERS: [&str; 8] = [
    "host",
    "content-length",
    "transfer-encoding",
    "connection",
    "keep-alive",
    "upgrade",
    "te",
    "trailer",
];

pub(crate) async fn handle(
    core: &Core,
    request: SecretsRequest,
) -> Result<Response, ProtocolError> {
    let secrets = core.secrets_store();
    match request {
        SecretsRequest::List { project } => {
            core.project_store().get(&project)?;
            let (store, secrets) = blocking(move || {
                let list = secrets.list(&project)?;
                Ok((secrets.status(), list))
            })
            .await?;
            Ok(Response::Secrets(SecretsResponse::List(SecretList {
                store,
                secrets,
            })))
        }
        SecretsRequest::Set {
            project,
            secret,
            value,
        } => {
            check(&secret, &value)?;
            let _changing = core.changing_secrets().await;
            core.project_store().get(&project)?;
            let changed = project.clone();
            let set = blocking(move || {
                let taken = secrets.list(&project)?.into_iter().find(|other| {
                    other.name != secret.name
                        && same_host(&other.host, &secret.host)
                        && other.header.eq_ignore_ascii_case(&secret.header)
                });
                if let Some(other) = taken {
                    return Err(ProtocolError::new(
                        ErrorCode::Conflict,
                        format!(
                            "{} already sends {} to {}",
                            other.name, other.header, other.host
                        ),
                    ));
                }
                secrets.set(&project, secret, value)
            })
            .await;
            crate::audit::forget_secrets(core, &changed);
            set?;
            core.emit(SecretsEvent::Changed { project: changed });
            Ok(Response::Ok(OkResponse::Ok))
        }
        SecretsRequest::Remove { project, name } => {
            let _changing = core.changing_secrets().await;
            core.project_store().get(&project)?;
            let changed = project.clone();
            let removed = blocking(move || secrets.remove(&project, &name)).await;
            crate::audit::forget_secrets(core, &changed);
            removed?;
            core.emit(SecretsEvent::Changed { project: changed });
            Ok(Response::Ok(OkResponse::Ok))
        }
    }
}

/// Removes the secrets of a project the person is removing; the caller
/// holds [`Core::changing_secrets`].
pub(crate) async fn remove_project(core: &Core, project: &ProjectId) -> Result<(), ProtocolError> {
    let secrets = core.secrets_store();
    let removed = project.clone();
    let result = blocking(move || secrets.remove_project(&removed)).await;
    crate::audit::forget_secrets(core, project);
    result
}

pub(crate) async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, ProtocolError> + Send + 'static,
) -> Result<T, ProtocolError> {
    tokio::task::spawn_blocking(f).await.map_err(|_| {
        ProtocolError::new(
            ErrorCode::Internal,
            "the keychain call stopped unexpectedly",
        )
    })?
}

/// Refuses a secret the netproxy could not send safely. Messages name the
/// field, never the value.
pub(crate) fn check(secret: &SecretInfo, value: &SecretValue) -> Result<(), ProtocolError> {
    let bad = |why: String| Err(ProtocolError::new(ErrorCode::BadRequest, why));
    if !name_ok(&secret.name) {
        return bad(format!(
            "a secret's name is 1 to {MAX_NAME} letters, digits, `_`, `.` or `-`, starting with a letter or digit"
        ));
    }
    if let Err(why) = host_ok(&secret.host) {
        return bad(format!("{}: {why}", secret.name));
    }
    if let Err(why) = header_ok(&secret.header) {
        return bad(format!("{}: {why}", secret.name));
    }
    if secret.prefix.len() > MAX_PREFIX || has_control(&secret.prefix) {
        return bad(format!(
            "{}: the prefix is at most {MAX_PREFIX} bytes, without control characters",
            secret.name
        ));
    }
    let value = value.expose();
    if value.is_empty() || value.len() > MAX_VALUE || has_control(value) {
        return bad(format!(
            "{}: the value is 1 to {MAX_VALUE} bytes, without control characters or line breaks",
            secret.name
        ));
    }
    Ok(())
}

fn name_ok(name: &str) -> bool {
    name.len() <= MAX_NAME
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

fn has_control(text: &str) -> bool {
    text.chars().any(char::is_control)
}

/// A lowercase DNS name with a dot, optionally `:port`; no IP addresses
/// or wildcards.
fn host_ok(host: &str) -> Result<(), &'static str> {
    let (name, port) = match host.split_once(':') {
        Some((name, port)) => (name, Some(port)),
        None => (host, None),
    };
    if let Some(port) = port {
        let digits =
            !port.is_empty() && port.len() <= 5 && port.bytes().all(|b| b.is_ascii_digit());
        if !digits || port.parse::<u16>().map_or(true, |p| p == 0) {
            return Err("the host's port is a number from 1 to 65535");
        }
    }
    let labels: Vec<&str> = name.split('.').collect();
    let label_ok = |l: &&str| {
        !l.is_empty()
            && l.len() <= 63
            && !l.starts_with('-')
            && !l.ends_with('-')
            && l.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    };
    if name.len() > 253 || labels.len() < 2 || !labels.iter().all(label_ok) {
        return Err(
            "the host is one lowercase DNS name with a dot, such as api.github.com, optionally with :port",
        );
    }
    if labels
        .last()
        .is_some_and(|l| l.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err("the host is a DNS name, not an IP address");
    }
    let claude = CLAUDE_DOMAINS
        .iter()
        .any(|d| name == *d || name.ends_with(&format!(".{d}")));
    let aws = name.ends_with(".amazonaws.com") || name.ends_with(".amazonaws.com.cn");
    let cloud_model = (aws && labels.iter().any(|l| l.starts_with("bedrock")))
        || name.ends_with("aiplatform.googleapis.com")
        || name.ends_with(".services.ai.azure.com");
    if claude || cloud_model {
        return Err("Claude's own hosts carry its login; a secret is never sent to them");
    }
    Ok(())
}

fn header_ok(header: &str) -> Result<(), &'static str> {
    let token = !header.is_empty()
        && header.len() <= MAX_HEADER
        && header
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b));
    if !token {
        return Err("the header is a header name, such as Authorization");
    }
    let lower = header.to_ascii_lowercase();
    if REFUSED_HEADERS.contains(&lower.as_str()) || lower.starts_with("proxy-") {
        return Err("that header frames or routes the request; a secret cannot replace it");
    }
    Ok(())
}

/// Whether two checked hosts name the same place; no port means 443.
fn same_host(a: &str, b: &str) -> bool {
    let full = |h: &str| {
        if h.contains(':') {
            h.to_owned()
        } else {
            format!("{h}:443")
        }
    };
    full(a) == full(b)
}

#[cfg(test)]
mod tests;
