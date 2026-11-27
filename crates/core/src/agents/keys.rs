//! Whether a session may use a key the netproxy would add to its request
//! (`docs/specs/secrets-consent.md`): the project's `UseKey` rules decide,
//! else the session's earlier answer, else the person is asked once.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::Core;
use crate::policy::keys::{self as rules, KEY_TOOL};
use crate::protocol::agents::{SessionId, ToolCall};
use crate::protocol::audit::DecidedBy;
use crate::protocol::permissions::Decision;
use crate::protocol::policy::PolicyAction;
use crate::protocol::projects::ProjectId;
use crate::protocol::secrets::SecretInfo;
use crate::services::{EventSink, KeyConsent};

use super::{AgentHost, ToolRequest, lock};

/// How long a held request waits for the person; then it goes without
/// the key, and the next one asks again.
pub(crate) const ASK_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// The questions being asked, by session and key.
/// Each gate holds whether its last question went unanswered.
pub(crate) type Asking = Mutex<HashMap<(SessionId, String), Arc<tokio::sync::Mutex<bool>>>>;

/// The consent the netproxy of `session` asks.
pub(crate) fn consent(
    core: crate::core::WeakCore,
    project: ProjectId,
    session: SessionId,
    sink: EventSink,
) -> KeyConsent {
    KeyConsent::new(move |secret| {
        let (core, project, session, sink) =
            (core.clone(), project.clone(), session.clone(), sink.clone());
        let secret = secret.clone();
        Box::pin(async move {
            let Some(core) = core.upgrade() else {
                return false;
            };
            let host = AgentHost::new(session, project, core.sessions().clone(), sink);
            host.use_key(&core, &secret).await
        })
    })
}

impl AgentHost {
    /// Whether the proxy may add `secret` to this session's request now.
    /// Fails closed: a key removed or rebound since the proxy started, a
    /// session that is gone, an unreadable policy that cannot ask.
    async fn use_key(&self, core: &Core, secret: &SecretInfo) -> bool {
        let store = core.secrets_store();
        let project = self.project.clone();
        let Ok(current) = crate::secrets::blocking(move || store.list(&project)).await else {
            return false;
        };
        if !current.iter().any(|s| s == secret) {
            return false;
        }
        let name = secret.name.as_str();
        let tool = ToolRequest {
            name: KEY_TOOL.to_owned(),
            input: serde_json::json!({ "key": name, "host": secret.host }),
            call: ToolCall::default(),
            written: None,
        };
        let Ok(sandbox) = self.sessions.find(&self.session, |s| s.info().sandbox) else {
            return false;
        };
        // An unreadable policy decides nothing: the person is asked.
        let decided = core
            .project_store()
            .policy(&self.project)
            .ok()
            .and_then(|p| rules::decide(&p, name, sandbox.as_ref()));
        match decided {
            Some((PolicyAction::Deny, rule)) => {
                self.audit_tool(&tool, false, DecidedBy::Policy { rule })
                    .await;
                return false;
            }
            Some((PolicyAction::Auto, rule)) => {
                self.audit_tool(&tool, true, DecidedBy::Policy { rule })
                    .await;
                return true;
            }
            _ => {}
        }
        let key = (self.session.clone(), name.to_owned());
        let gate = lock(&self.sessions.asking_keys)
            .entry(key.clone())
            .or_default()
            .clone();
        let mut unanswered = gate.lock().await;
        if let Ok(Some(answer)) = self.sessions.find(&self.session, |s| s.key_answer(secret)) {
            return answer;
        }
        // The question before this one went unanswered: requests that
        // waited for it go without the key rather than ask in turn.
        if *unanswered {
            return false;
        }
        let _asking = Asked {
            asking: &self.sessions.asking_keys,
            key,
        };
        let title = format!("Use your {name} key on {}", secret.host);
        let always =
            |_: Option<&std::path::Path>, sandbox: Option<&_>| rules::always(name, sandbox);
        let asked = tokio::time::timeout(
            ASK_TIMEOUT,
            self.ask_person_by(&tool, Some(title), None, always, true),
        )
        .await;
        let Ok((answer, by)) = asked else {
            *unanswered = true;
            return false;
        };
        let allowed = answer.decision == Decision::Allow;
        // Only a person's answer on the card holds for the session: a deny
        // because the turn was interrupted or the session stopped asks
        // again next time.
        let interrupted = answer.message.as_deref() == Some(super::requests::INTERRUPTED);
        if matches!(by, DecidedBy::Person | DecidedBy::Phone { .. }) && !interrupted {
            let _ = self
                .sessions
                .find(&self.session, |s| s.set_key_answer(secret, allowed));
        }
        allowed
    }
}

/// Takes a finished question out of [`Asking`], also when the request that
/// asked it is dropped. The answer is kept first, so a request that comes
/// after finds it.
struct Asked<'a> {
    asking: &'a Asking,
    key: (SessionId, String),
}

impl Drop for Asked<'_> {
    fn drop(&mut self) {
        lock(self.asking).remove(&self.key);
    }
}

#[cfg(test)]
mod tests;
