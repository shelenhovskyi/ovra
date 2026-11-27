//! Plan first (`docs/specs/plan-first.md`): the agent proposes a plan and
//! changes nothing until the person approves it. Ovra holds the line
//! itself, whatever the agent does: while a session plans, file changes
//! are refused and every command goes to the person.

use crate::protocol::agents::{AgentStatus, AgentsEvent, MessageFrom, SessionId, ToolKind};
use crate::protocol::{ErrorCode, ProtocolError};

use super::requests::Ctx;
use super::{AgentHost, ToolRequest};

/// What the agent reads after the person's first message.
pub(super) const NOTE: &str = "Plan first: the person wants to see your plan before anything \
     changes. Look at what you need, then reply with a short numbered plan and end your turn. \
     Until the person approves the plan, Ovra refuses file changes and asks the person before \
     each command; it tells you when you may go ahead.";

/// What the agent reads when the person approves.
const APPROVED: &str = "The person approved your plan. Carry it out now; you may change files.";

/// Why a planning session may not make the call.
const REFUSED: &str = "Plan first: the person has not approved your plan yet, so Ovra refuses \
     file changes. Finish the plan, end your turn and wait for their answer.";

/// Why a planning session's command goes to the person.
pub(super) const ASKS: &str =
    "Plan first: the agent wants to run this before you approved its plan.";

/// Ovra's IDE tools that run the project's commands, in a pod that can
/// write its folder: refused while a session plans.
const RUNS_COMMANDS: [&str; 2] = ["server_start", "server_restart"];

/// What plan first does to one tool call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Hold {
    /// Not planning, or a call that changes nothing here.
    None,
    /// The person decides, whatever a rule or setting allows.
    Ask,
    Refuse,
}

impl AgentHost {
    /// What plan first does to `tool`. A session that is gone holds
    /// nothing here: the research and stopped checks refuse it first.
    pub(super) fn plan_hold(&self, tool: &ToolRequest) -> Hold {
        let planning = self
            .sessions
            .find(&self.session, |s| s.info().planning)
            .unwrap_or(false);
        if !planning {
            return Hold::None;
        }
        // Ovra's own tools follow their role rules, except those that run
        // the project's commands.
        if let Some(name) = tool
            .name
            .strip_prefix(crate::orchestration::IDE_TOOL_PREFIX)
        {
            return if RUNS_COMMANDS.contains(&name) {
                Hold::Refuse
            } else {
                Hold::None
            };
        }
        // Fails closed: a tool Ovra does not know goes to the person.
        match tool.call.kind {
            ToolKind::Read
            | ToolKind::Search
            | ToolKind::Think
            | ToolKind::Fetch
            | ToolKind::Subagent => Hold::None,
            ToolKind::Edit | ToolKind::Delete | ToolKind::Move => Hold::Refuse,
            ToolKind::Execute | ToolKind::Other => Hold::Ask,
        }
    }

    /// The refusal for [`Hold::Refuse`].
    pub(super) fn plan_refusal() -> String {
        REFUSED.to_owned()
    }
}

/// `agents.approve_plan`: the session may change files again, and the agent
/// hears so in Ovra's words.
pub(super) fn approve(ctx: &Ctx<'_>, session: &SessionId) -> Result<(), ProtocolError> {
    let approved = ctx.sessions.find(session, |s| {
        let info = s.info();
        if !info.planning {
            return Err(ProtocolError::new(
                ErrorCode::Conflict,
                "the agent is not waiting for a plan to be approved",
            ));
        }
        if info.status != AgentStatus::Idle || s.stopping() {
            return Err(ProtocolError::new(
                ErrorCode::Conflict,
                "the agent is still working; approve the plan once its turn ends",
            ));
        }
        s.set_planning(false);
        Ok(())
    })?;
    approved?;
    planning_changed(ctx, session, false);
    let sent = super::requests::send(
        ctx,
        session.clone(),
        vec![(APPROVED.to_owned(), MessageFrom::Ovra)],
    );
    if sent.is_err() {
        // The process went away in between: it still plans.
        let _ = ctx.sessions.find(session, |s| s.set_planning(true));
        planning_changed(ctx, session, true);
    }
    sent
}

fn planning_changed(ctx: &Ctx<'_>, session: &SessionId, planning: bool) {
    ctx.sink.emit(AgentsEvent::Planning {
        session: session.clone(),
        planning,
    });
}
