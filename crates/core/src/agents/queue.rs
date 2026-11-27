//! Messages the person queues while a turn runs (`agents.queue`,
//! `docs/specs/plan-first.md`): each goes as the next message once a turn
//! ends, instead of interrupting the agent.

use crate::protocol::agents::{AgentStatus, AgentsEvent, MAX_QUEUED, QueuedMessage, SessionId};
use crate::protocol::{ErrorCode, ProtocolError};
use crate::services::AgentImage;

use super::requests::{Ctx, Parts};
use super::{AgentHost, SessionView};

/// One session's queue.
#[derive(Debug, Default)]
pub(super) struct Later {
    /// The last id given out.
    made: u32,
    messages: Vec<Waiting>,
}

/// A queued message, with the context the person added to it, as it was
/// when they queued it.
#[derive(Debug)]
struct Waiting {
    id: u32,
    parts: Parts,
    images: Vec<AgentImage>,
}

impl Later {
    /// What clients see.
    pub(super) fn shown(&self) -> Vec<QueuedMessage> {
        self.messages
            .iter()
            .map(|m| QueuedMessage {
                id: m.id,
                text: m.parts.first().map(|(t, _)| t.clone()).unwrap_or_default(),
            })
            .collect()
    }
}

impl SessionView<'_> {
    /// Adds `parts` to the queue of a session whose turn runs or that
    /// waits to start; `Ok(false)` for an idle session, which takes them
    /// now.
    fn queue(&mut self, parts: &Parts, images: &[AgentImage]) -> Result<bool, ProtocolError> {
        let status = self.status().clone();
        if self.stopping() || status.is_ended() {
            return Err(conflict("the session has ended"));
        }
        if status == AgentStatus::Idle {
            return Ok(false);
        }
        let later = &mut self.session.later;
        if later.messages.len() >= MAX_QUEUED {
            return Err(conflict(&format!(
                "{MAX_QUEUED} messages already wait; remove one or wait for the agent"
            )));
        }
        later.made += 1;
        later.messages.push(Waiting {
            id: later.made,
            parts: parts.clone(),
            images: images.to_vec(),
        });
        Ok(true)
    }

    fn unqueue(&mut self, id: u32) -> bool {
        let messages = &mut self.session.later.messages;
        let before = messages.len();
        messages.retain(|m| m.id != id);
        messages.len() != before
    }

    /// The oldest message, once its session is idle and not stopping.
    fn next_queued(&mut self) -> Option<(Parts, Vec<AgentImage>)> {
        if self.stopping() || *self.status() != AgentStatus::Idle {
            return None;
        }
        let messages = &mut self.session.later.messages;
        (!messages.is_empty()).then(|| {
            let first = messages.remove(0);
            (first.parts, first.images)
        })
    }

    /// An ended session takes no more messages.
    pub(super) fn drop_queue(&mut self) -> bool {
        let had = !self.session.later.messages.is_empty();
        self.session.later.messages.clear();
        had
    }
}

/// `agents.queue`: `parts` wait for the running turn, or go now to an idle
/// session.
pub(super) fn queue(
    ctx: &Ctx<'_>,
    session: &SessionId,
    parts: Parts,
    images: Vec<AgentImage>,
) -> Result<(), ProtocolError> {
    super::requests::check_parts(&parts)?;
    let queued = ctx.sessions.find(session, |s| s.queue(&parts, &images))??;
    if queued {
        changed(ctx, session);
        Ok(())
    } else {
        super::requests::send_parts(ctx, session.clone(), parts, images)
    }
}

/// `agents.unqueue`.
pub(super) fn unqueue(ctx: &Ctx<'_>, session: &SessionId, id: u32) -> Result<(), ProtocolError> {
    if !ctx.sessions.find(session, |s| s.unqueue(id))? {
        return Err(ProtocolError::new(
            ErrorCode::NotFound,
            "the message was already sent",
        ));
    }
    changed(ctx, session);
    Ok(())
}

/// Tells clients the session's queue.
fn changed(ctx: &Ctx<'_>, session: &SessionId) {
    if let Ok(queue) = ctx.sessions.find(session, |s| s.session.later.shown()) {
        ctx.sink.emit(AgentsEvent::Queue {
            session: session.clone(),
            queue,
        });
    }
}

impl AgentHost {
    /// A turn ended: the oldest queued message, if any, starts the next.
    pub(super) fn send_queued(&self) {
        let Some(core) = self.sessions.core() else {
            return;
        };
        let ctx = core.agents_ctx();
        let Ok(Some((parts, images))) = self.sessions.find(&self.session, |s| s.next_queued())
        else {
            return;
        };
        changed(&ctx, &self.session);
        // Checked when queued; a session that ended meanwhile refuses, and
        // the transcript says what did not go.
        if let Err(err) =
            super::requests::send_parts(&ctx, self.session.clone(), parts.clone(), images)
        {
            let text = parts.first().map(|(t, _)| t.as_str()).unwrap_or_default();
            let line: String = text.lines().next().unwrap_or("").chars().take(80).collect();
            self.message(crate::protocol::agents::AgentMessage::Error {
                message: format!("A queued message was not sent ({}): {line}", err.message),
            });
        }
    }

    /// The session ended: what waited for it is dropped.
    pub(super) fn drop_queue(&self) {
        if self.sessions.find(&self.session, |s| s.drop_queue()) == Ok(true) {
            self.sink.emit(AgentsEvent::Queue {
                session: self.session.clone(),
                queue: Vec::new(),
            });
        }
    }
}

fn conflict(message: &str) -> ProtocolError {
    ProtocolError::new(ErrorCode::Conflict, message)
}
