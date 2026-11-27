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
pub(crate) type Asking = Mutex<HashMap<(SessionId, String), Arc<tokio::sync::Mutex<()>>>>;

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
        let _held = gate.lock().await;
        if let Ok(Some(answer)) = self.sessions.find(&self.session, |s| s.key_answer(name)) {
            return answer;
        }
        let title = format!("Use your {name} key on {}", secret.host);
        let always =
            |_: Option<&std::path::Path>, sandbox: Option<&_>| rules::always(name, sandbox);
        let asked = tokio::time::timeout(
            ASK_TIMEOUT,
            self.ask_person(&tool, Some(title), None, always),
        )
        .await;
        lock(&self.sessions.asking_keys).remove(&key);
        let Ok(answer) = asked else {
            return false;
        };
        let allowed = answer.decision == Decision::Allow;
        let _ = self
            .sessions
            .find(&self.session, |s| s.set_key_answer(name, allowed));
        allowed
    }
}

#[cfg(test)]
mod tests;
