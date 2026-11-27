//! Messages exchanged between a front end and the core.
//!
//! The protocol is split by domain so parallel work touches different files.
//! Each domain has its own request, response and event enums whose tags are
//! namespaced (`fs.list_dir`, `projects.changed`, ...); the top-level enums
//! below only combine them. Keep `ui/src/lib/protocol/` in sync.

pub mod agents;
pub mod audit;
pub mod board;
pub mod browser;
pub mod catalog;
pub mod context;
pub mod fs;
pub mod git;
pub mod info;
pub mod lsp;
pub mod netproxy;
pub mod open;
pub mod orchestration;
pub mod permissions;
pub mod policy;
pub mod power;
pub mod pr;
pub mod projects;
pub mod remote;
pub mod resources;
pub mod review;
pub mod schedules;
pub mod search;
pub mod secrets;
pub mod servers;
pub mod setup;
pub mod ssh;
pub mod terminals;

use serde::{Deserialize, Serialize};

/// A request from a front end. Wire form: `{"method": "<domain>.<name>", "params": ...}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Request {
    Info(info::InfoRequest),
    Projects(projects::ProjectsRequest),
    Fs(fs::FsRequest),
    Open(open::OpenRequest),
    Terminals(terminals::TerminalsRequest),
    Agents(agents::AgentsRequest),
    Permissions(permissions::PermissionsRequest),
    Review(review::ReviewRequest),
    Orchestration(orchestration::OrchestrationRequest),
    Board(board::BoardRequest),
    Search(search::SearchRequest),
    Policy(policy::PolicyRequest),
    Secrets(secrets::SecretsRequest),
    Audit(audit::AuditRequest),
    Netproxy(netproxy::NetproxyRequest),
    Git(git::GitRequest),
    Lsp(lsp::LspRequest),
    Servers(servers::ServersRequest),
    Browser(browser::BrowserRequest),
    Pr(pr::PrRequest),
    Power(power::PowerRequest),
    Context(context::ContextRequest),
    Remote(remote::RemoteRequest),
    Setup(setup::SetupRequest),
    Catalog(catalog::CatalogRequest),
    Ssh(ssh::SshRequest),
    Schedules(schedules::SchedulesRequest),
}

/// The core's answer to a [`Request`]. Wire form: `{"type": "<domain>.<name>", "data": ...}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Response {
    Info(info::InfoResponse),
    Projects(projects::ProjectsResponse),
    Fs(fs::FsResponse),
    Terminals(terminals::TerminalsResponse),
    Agents(agents::AgentsResponse),
    Permissions(permissions::PermissionsResponse),
    Review(review::ReviewResponse),
    Orchestration(orchestration::OrchestrationResponse),
    Board(board::BoardResponse),
    Search(search::SearchResponse),
    Policy(policy::PolicyResponse),
    Secrets(secrets::SecretsResponse),
    Audit(audit::AuditResponse),
    Netproxy(netproxy::NetproxyResponse),
    Git(git::GitResponse),
    Lsp(lsp::LspResponse),
    Servers(servers::ServersResponse),
    Browser(browser::BrowserResponse),
    Pr(pr::PrResponse),
    Power(power::PowerResponse),
    Context(context::ContextResponse),
    Remote(remote::RemoteResponse),
    Setup(setup::SetupResponse),
    Catalog(catalog::CatalogResponse),
    Ssh(ssh::SshResponse),
    Schedules(schedules::SchedulesResponse),
    /// The request succeeded and has nothing to return.
    Ok(OkResponse),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OkResponse {
    Ok,
}

/// A notification pushed by the core. Wire form: `{"type": "<domain>.<name>", "data": ...}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Event {
    Info(info::InfoEvent),
    Projects(projects::ProjectsEvent),
    Fs(fs::FsEvent),
    Terminals(terminals::TerminalsEvent),
    Agents(agents::AgentsEvent),
    Permissions(permissions::PermissionsEvent),
    Review(review::ReviewEvent),
    Orchestration(orchestration::OrchestrationEvent),
    Board(board::BoardEvent),
    Search(search::SearchEvent),
    Policy(policy::PolicyEvent),
    Secrets(secrets::SecretsEvent),
    Audit(audit::AuditLogEvent),
    Netproxy(netproxy::NetproxyEvent),
    Lsp(lsp::LspEvent),
    Servers(servers::ServersEvent),
    Browser(browser::BrowserEvent),
    Pr(pr::PrEvent),
    Resources(resources::ResourcesEvent),
    Power(power::PowerEvent),
    Remote(remote::RemoteEvent),
    Ssh(ssh::SshEvent),
    Schedules(schedules::SchedulesEvent),
}

/// An error returned instead of a [`Response`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolError {
    pub code: ErrorCode,
    pub message: String,
}

impl ProtocolError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}

impl std::error::Error for ProtocolError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The message could not be parsed or has invalid parameters.
    BadRequest,
    /// The project, file or other target does not exist.
    NotFound,
    /// The target changed since the client last saw it.
    Conflict,
    /// A path points outside its project.
    Forbidden,
    /// The feature is not wired into this front end (yet).
    Unavailable,
    /// Anything else; the message has details.
    Internal,
}

impl From<info::InfoEvent> for Event {
    fn from(e: info::InfoEvent) -> Self {
        Self::Info(e)
    }
}

impl From<projects::ProjectsEvent> for Event {
    fn from(e: projects::ProjectsEvent) -> Self {
        Self::Projects(e)
    }
}

impl From<fs::FsEvent> for Event {
    fn from(e: fs::FsEvent) -> Self {
        Self::Fs(e)
    }
}

impl From<terminals::TerminalsEvent> for Event {
    fn from(e: terminals::TerminalsEvent) -> Self {
        Self::Terminals(e)
    }
}

impl From<agents::AgentsEvent> for Event {
    fn from(e: agents::AgentsEvent) -> Self {
        Self::Agents(e)
    }
}

impl From<permissions::PermissionsEvent> for Event {
    fn from(e: permissions::PermissionsEvent) -> Self {
        Self::Permissions(e)
    }
}

impl From<review::ReviewEvent> for Event {
    fn from(e: review::ReviewEvent) -> Self {
        Self::Review(e)
    }
}

impl From<board::BoardEvent> for Event {
    fn from(e: board::BoardEvent) -> Self {
        Self::Board(e)
    }
}

impl From<schedules::SchedulesEvent> for Event {
    fn from(e: schedules::SchedulesEvent) -> Self {
        Self::Schedules(e)
    }
}

impl From<orchestration::OrchestrationEvent> for Event {
    fn from(e: orchestration::OrchestrationEvent) -> Self {
        Self::Orchestration(e)
    }
}

impl From<policy::PolicyEvent> for Event {
    fn from(e: policy::PolicyEvent) -> Self {
        Self::Policy(e)
    }
}

impl From<secrets::SecretsEvent> for Event {
    fn from(e: secrets::SecretsEvent) -> Self {
        Self::Secrets(e)
    }
}

impl From<search::SearchEvent> for Event {
    fn from(e: search::SearchEvent) -> Self {
        Self::Search(e)
    }
}

impl From<audit::AuditLogEvent> for Event {
    fn from(e: audit::AuditLogEvent) -> Self {
        Self::Audit(e)
    }
}

impl From<netproxy::NetproxyEvent> for Event {
    fn from(e: netproxy::NetproxyEvent) -> Self {
        Self::Netproxy(e)
    }
}

impl From<lsp::LspEvent> for Event {
    fn from(e: lsp::LspEvent) -> Self {
        Self::Lsp(e)
    }
}

impl From<servers::ServersEvent> for Event {
    fn from(e: servers::ServersEvent) -> Self {
        Self::Servers(e)
    }
}

impl From<browser::BrowserEvent> for Event {
    fn from(e: browser::BrowserEvent) -> Self {
        Self::Browser(e)
    }
}

impl From<pr::PrEvent> for Event {
    fn from(e: pr::PrEvent) -> Self {
        Self::Pr(e)
    }
}

impl From<resources::ResourcesEvent> for Event {
    fn from(e: resources::ResourcesEvent) -> Self {
        Self::Resources(e)
    }
}

impl From<power::PowerEvent> for Event {
    fn from(e: power::PowerEvent) -> Self {
        Self::Power(e)
    }
}

impl From<remote::RemoteEvent> for Event {
    fn from(e: remote::RemoteEvent) -> Self {
        Self::Remote(e)
    }
}

impl From<ssh::SshEvent> for Event {
    fn from(e: ssh::SshEvent) -> Self {
        Self::Ssh(e)
    }
}
