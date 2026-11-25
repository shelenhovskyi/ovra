//! What the `server_*` IDE tools do, for `ovra-mcp-server` (task S.E).
//! Every role gets them, each for the caller's own work dir only: a worker
//! its worktree, any other session the main checkout.

use std::time::Duration;

use super::{DEFAULT_GRACE, Entry, Result, STOP_SLACK, find, not_found, place};
use crate::Core;
use crate::protocol::servers::{
    LogPage, ServerId, ServerInfo, ServerList, ServerStatus, StartedBy,
};
use crate::services::ToolCaller;

/// The longest a start or restart waits for its server.
const MAX_SERVER_WAIT: Duration = Duration::from_secs(60);

impl Core {
    /// `server_list`: the definitions and servers of the caller's work dir.
    pub async fn server_list(&self, caller: &ToolCaller) -> Result<ServerList> {
        let place = place(self, &caller.project, Some(&caller.session))?;
        Ok(super::list(self, &place).await)
    }

    /// `server_start`: starts the defined server `name`, then waits up to
    /// `wait` (at most [`MAX_SERVER_WAIT`]) until it is ready or ended.
    pub async fn server_start(
        &self,
        caller: &ToolCaller,
        name: &str,
        wait: Duration,
    ) -> Result<ServerInfo> {
        let place = place(self, &caller.project, Some(&caller.session))?;
        let info = super::start(self, place, name, started_by(caller)).await?;
        settled(self, &info, wait).await
    }

    /// `server_stop`: stops a server and waits for its end.
    pub async fn server_stop(&self, caller: &ToolCaller, id: &ServerId) -> Result<ServerInfo> {
        owned(self, caller, id)?;
        super::stop(self, id, DEFAULT_GRACE)?;
        super::wait(self, id, DEFAULT_GRACE + STOP_SLACK, |e| !e.running()).await
    }

    /// `server_restart`: stop, then start with a new generation, waiting
    /// like [`Core::server_start`].
    pub async fn server_restart(
        &self,
        caller: &ToolCaller,
        id: &ServerId,
        wait: Duration,
    ) -> Result<ServerInfo> {
        owned(self, caller, id)?;
        let info = super::restart(self, id, started_by(caller)).await?;
        settled(self, &info, wait).await
    }

    /// `server_logs`: up to `limit` lines above `since` containing `grep`,
    /// without ANSI escapes. The text is the server's, which project code
    /// writes: data for the agent, never instructions.
    pub fn server_logs(
        &self,
        caller: &ToolCaller,
        id: &ServerId,
        since: u64,
        limit: usize,
        grep: Option<&str>,
    ) -> Result<LogPage> {
        owned(self, caller, id)?;
        let state = self.dev_servers().lock();
        Ok(find(&state, id)?.log.page(since, Some(limit), grep, true))
    }
}

fn started_by(caller: &ToolCaller) -> StartedBy {
    StartedBy::Agent {
        session: caller.session.clone(),
    }
}

/// Waits until this start of the server is no longer `starting`.
async fn settled(core: &Core, info: &ServerInfo, wait: Duration) -> Result<ServerInfo> {
    let generation = info.generation;
    let done = |e: &Entry| e.generation != generation || e.status != ServerStatus::Starting;
    super::wait(core, &info.id, wait.min(MAX_SERVER_WAIT), done).await
}

/// DevServerState of other work dirs do not exist for the caller.
fn owned(core: &Core, caller: &ToolCaller, id: &ServerId) -> Result<()> {
    let place = place(core, &caller.project, Some(&caller.session))?;
    let state = core.dev_servers().lock();
    match find(&state, id) {
        Ok(entry) if entry.place == place => Ok(()),
        _ => Err(not_found(format!("server {}", id.0))),
    }
}
