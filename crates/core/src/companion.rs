//! What a paired phone may do (ADR 0014, `docs/specs/companion.md`): an
//! explicit list of requests and events. Matches name every variant of the
//! domains a phone may use, so a new method there fails to compile until
//! someone decides; other domains are refused as a whole.

use crate::protocol::agents::{AgentsEvent, AgentsRequest};
use crate::protocol::info::{InfoEvent, InfoRequest};
use crate::protocol::orchestration::{OrchestrationEvent, OrchestrationRequest};
use crate::protocol::permissions::{PermissionsEvent, PermissionsRequest};
use crate::protocol::power::{PowerEvent, PowerRequest};
use crate::protocol::projects::ProjectsRequest;
use crate::protocol::review::{ReviewEvent, ReviewRequest};
use crate::protocol::{Event, Request};

/// Whether a phone may send `request`.
pub(crate) fn allows(request: &Request) -> bool {
    match request {
        Request::Info(InfoRequest::Get)
        | Request::Power(PowerRequest::Get | PowerRequest::SetMode { .. }) => true,
        Request::Projects(r) => match r {
            ProjectsRequest::List => true,
            ProjectsRequest::Add { .. }
            | ProjectsRequest::Remove { .. }
            | ProjectsRequest::Open { .. }
            | ProjectsRequest::Trust { .. }
            | ProjectsRequest::GetState { .. }
            | ProjectsRequest::SetState { .. } => false,
        },
        Request::Agents(r) => match r {
            // A phone sends plain text; context items (selections, terminal
            // text, confirmed secrets) are attached only on the computer.
            AgentsRequest::Send { context, .. } => context.is_empty(),
            AgentsRequest::List { .. }
            | AgentsRequest::Interrupt { .. }
            | AgentsRequest::Stop { .. }
            | AgentsRequest::Transcript { .. }
            | AgentsRequest::Capacity { .. }
            | AgentsRequest::Limit => true,
            AgentsRequest::Providers
            | AgentsRequest::Setup { .. }
            | AgentsRequest::SetPath { .. }
            | AgentsRequest::Accounts { .. }
            | AgentsRequest::SaveAccount { .. }
            | AgentsRequest::RemoveAccount { .. }
            | AgentsRequest::SetProjectAccount { .. }
            | AgentsRequest::ProjectConfig { .. }
            // Past tasks of every project stay on the computer.
            | AgentsRequest::History { .. }
            | AgentsRequest::Start { .. }
            | AgentsRequest::Resume { .. }
            | AgentsRequest::SetLimit { .. }
            | AgentsRequest::ResearchLimit
            | AgentsRequest::SetResearchLimit { .. }
            | AgentsRequest::Checkpoints { .. }
            | AgentsRequest::GoBack { .. }
            | AgentsRequest::UndoGoBack { .. } => false,
        },
        Request::Permissions(r) => match r {
            PermissionsRequest::Pending => true,
            // "Always allow" changes the project's policy: only on the computer.
            PermissionsRequest::Answer { always, .. } => !always,
        },
        Request::Orchestration(r) => match r {
            OrchestrationRequest::List { .. } => true,
            OrchestrationRequest::Start { .. }
            | OrchestrationRequest::Stop { .. }
            | OrchestrationRequest::RetrySetup { .. }
            | OrchestrationRequest::Leftovers { .. }
            | OrchestrationRequest::RemoveLeftover { .. } => false,
        },
        Request::Review(r) => match r {
            ReviewRequest::List { .. } => true,
            ReviewRequest::Diff { .. }
            | ReviewRequest::CheckLog { .. }
            | ReviewRequest::RunCheck { .. }
            | ReviewRequest::Warnings { .. }
            | ReviewRequest::Approve { .. }
            | ReviewRequest::Reject { .. }
            | ReviewRequest::UndoReject { .. }
            | ReviewRequest::Resolve { .. }
            | ReviewRequest::Take { .. }
            | ReviewRequest::UndoTake { .. }
            | ReviewRequest::DiscardHunk { .. }
            | ReviewRequest::UndoDiscard { .. }
            | ReviewRequest::Comment { .. }
            | ReviewRequest::Update { .. } => false,
        },
        Request::Fs(_)
        | Request::Open(_)
        | Request::Terminals(_)
        | Request::Search(_)
        | Request::Policy(_)
        | Request::Secrets(_)
        | Request::Audit(_)
        | Request::Netproxy(_)
        | Request::Git(_)
        | Request::Lsp(_)
        | Request::Servers(_)
        | Request::Browser(_)
        | Request::Pr(_)
        | Request::Context(_)
        | Request::Board(_)
        | Request::Remote(_)
        | Request::Setup(_)
        | Request::Catalog(_)
        | Request::Ssh(_) => false,
    }
}

/// Whether a phone gets `event`.
pub(crate) fn shows(event: &Event) -> bool {
    match event {
        Event::Info(InfoEvent::Ready | InfoEvent::Resync { .. })
        | Event::Permissions(PermissionsEvent::Requested(_) | PermissionsEvent::Resolved { .. })
        | Event::Orchestration(OrchestrationEvent::Changed { .. })
        | Event::Review(ReviewEvent::Changed { .. })
        | Event::Power(PowerEvent::Changed(_)) => true,
        // What the agent asks to show is for the desktop. A phone hears of
        // a review request through push and re-reads on `review.changed`.
        Event::Orchestration(OrchestrationEvent::Show { .. })
        | Event::Review(ReviewEvent::Requested { .. }) => false,
        Event::Agents(e) => match e {
            AgentsEvent::LimitChanged(_)
            | AgentsEvent::Message { .. }
            | AgentsEvent::Status { .. }
            | AgentsEvent::Billing { .. }
            | AgentsEvent::Sandbox { .. } => true,
            AgentsEvent::ResearchLimitChanged(_) | AgentsEvent::TextDelta { .. } => false,
        },
        Event::Projects(_)
        | Event::Resources(_)
        | Event::Fs(_)
        | Event::Terminals(_)
        | Event::Search(_)
        | Event::Policy(_)
        | Event::Secrets(_)
        | Event::Audit(_)
        | Event::Netproxy(_)
        | Event::Lsp(_)
        | Event::Servers(_)
        | Event::Browser(_)
        | Event::Pr(_)
        | Event::Board(_)
        | Event::Remote(_)
        | Event::Ssh(_) => false,
    }
}

#[cfg(test)]
mod tests;
