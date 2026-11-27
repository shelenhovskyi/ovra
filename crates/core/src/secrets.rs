//! `secrets.*` (`docs/specs/secrets.md`): the core checks every field, so
//! each `Secrets` implementation stores only what the netproxy can send
//! safely, then runs the keychain call on a blocking thread.

use crate::Core;
use crate::protocol::agents::AgentKind;
use crate::protocol::projects::ProjectId;
use crate::protocol::secrets::{
    SecretInfo, SecretList, SecretValue, SecretsEvent, SecretsRequest, SecretsResponse,
};
use crate::protocol::{ErrorCode, OkResponse, ProtocolError, Response};

const MAX_NAME: usize = 64;
const MAX_HEADER: usize = 64;
const MAX_PREFIX: usize = 64;
const MAX_VALUE: usize = 16 * 1024;

/// Every provider whose own hosts no secret is sent to. A new provider
/// goes here and in [`own_host`].
const PROVIDERS: [AgentKind; 3] = [AgentKind::Claude, AgentKind::Gemini, AgentKind::Codex];

/// Domains `claude` itself talks to; the proxy must never decrypt them
/// (ADR 0006). Their subdomains are refused too, and so are the cloud
/// providers' model endpoints it can use ([`own_host`]).
const CLAUDE_DOMAINS: [&str; 3] = ["anthropic.com", "claude.ai", "claude.com"];
/// Google's hosts a Gemini CLI login or API key goes to: the Gemini API,
/// Code Assist, sign-in and token refresh.
const GEMINI_HOSTS: [&str; 5] = [
    "generativelanguage.googleapis.com",
    "cloudcode-pa.googleapis.com",
    "oauth2.googleapis.com",
    "www.googleapis.com",
    "accounts.google.com",
];
/// OpenAI's domains a Codex login or API key goes to.
const CODEX_DOMAINS: [&str; 3] = ["openai.com", "chatgpt.com", "oaistatic.com"];

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
                let (same, others): (Vec<SecretInfo>, Vec<SecretInfo>) = secrets
                    .list(&project)?
                    .into_iter()
                    .partition(|other| other.name == secret.name);
                let rebound = same.iter().any(|old| {
                    old.host != secret.host
                        || old.header != secret.header
                        || old.prefix != secret.prefix
                });
                let taken = others.iter().find(|other| {
                    same_host(&other.host, &secret.host)
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
                let variable = secret.variable.as_ref();
                if let Some(other) = others
                    .iter()
                    .find(|other| variable.is_some() && other.variable.as_ref() == variable)
                {
                    return Err(ProtocolError::new(
                        ErrorCode::Conflict,
                        format!(
                            "{} already sets {}",
                            other.name,
                            variable.map_or("", String::as_str)
                        ),
                    ));
                }
                let name = secret.name.clone();
                secrets.set(&project, secret, value)?;
                Ok(rebound.then_some(name))
            })
            .await;
            crate::audit::forget_secrets(core, &changed);
            if let Some(name) = set? {
                crate::policy::forget_key(core, &changed, &name)?;
            }
            core.emit(SecretsEvent::Changed { project: changed });
            Ok(Response::Ok(OkResponse::Ok))
        }
        SecretsRequest::Remove { project, name } => {
            let _changing = core.changing_secrets().await;
            core.project_store().get(&project)?;
            let changed = project.clone();
            let forgotten = name.clone();
            let removed = blocking(move || secrets.remove(&project, &name)).await;
            crate::audit::forget_secrets(core, &changed);
            removed?;
            crate::policy::forget_key(core, &changed, &forgotten)?;
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
    if let Some(variable) = &secret.variable
        && !variable_ok(variable)
    {
        return bad(format!(
            "{}: the variable is up to {MAX_NAME} of `A-Z 0-9 _`, ends in _TOKEN, _KEY, _SECRET or _PAT, and is not one an agent's own login reads",
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

/// A placeholder variable may end only so: a name for a token.
const VARIABLE_ENDINGS: [&str; 4] = ["_TOKEN", "_KEY", "_SECRET", "_PAT"];
/// Variables agents' own logins and Ovra read: a placeholder there would
/// replace a real credential (`docs/specs/secrets-consent.md`).
const AGENT_VARIABLES: [&str; 9] = [
    "ANTHROPIC_",
    "CLAUDE_",
    "OPENAI_",
    "GEMINI_",
    "GOOGLE_",
    "AWS_",
    "AZURE_",
    "CODEX_",
    "OVRA_",
];

fn variable_ok(variable: &str) -> bool {
    variable.len() <= MAX_NAME
        && variable.starts_with(|c: char| c.is_ascii_uppercase())
        && variable
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        && VARIABLE_ENDINGS
            .iter()
            .any(|e| variable.ends_with(e) && variable.len() > e.len())
        && !AGENT_VARIABLES.iter().any(|p| variable.starts_with(p))
}

pub(crate) fn name_ok(name: &str) -> bool {
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
    // A project's secrets reach every session of it, whatever its
    // provider: no provider's own host takes one.
    match PROVIDERS
        .into_iter()
        .find_map(|agent| own_host(agent, name, &labels))
    {
        Some(why) => Err(why),
        None => Ok(()),
    }
}

/// Why `name` (split into `labels`) is one of `agent`'s own hosts, which
/// carry its login, if it is: the proxy never decrypts them (ADR 0006,
/// ADR 0011). Their subdomains count too.
fn own_host(agent: AgentKind, name: &str, labels: &[&str]) -> Option<&'static str> {
    let own = match agent {
        AgentKind::Claude => {
            let claude = CLAUDE_DOMAINS
                .iter()
                .any(|d| name == *d || name.ends_with(&format!(".{d}")));
            let aws = name.ends_with(".amazonaws.com") || name.ends_with(".amazonaws.com.cn");
            let cloud_model = (aws && labels.iter().any(|l| l.starts_with("bedrock")))
                || name.ends_with("aiplatform.googleapis.com")
                || name.ends_with(".services.ai.azure.com");
            claude || cloud_model
        }
        AgentKind::Gemini => {
            GEMINI_HOSTS
                .iter()
                .any(|h| name == *h || name.ends_with(&format!(".{h}")))
                || name.ends_with("aiplatform.googleapis.com")
        }
        AgentKind::Codex => CODEX_DOMAINS
            .iter()
            .any(|d| name == *d || name.ends_with(&format!(".{d}"))),
    };
    own.then_some(match agent {
        AgentKind::Claude => "Claude's own hosts carry its login; a secret is never sent to them",
        AgentKind::Gemini => "Gemini's own hosts carry its login; a secret is never sent to them",
        AgentKind::Codex => "Codex's own hosts carry its login; a secret is never sent to them",
    })
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
