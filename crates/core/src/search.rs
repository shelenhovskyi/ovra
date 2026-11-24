//! `search.*`: content searches run by the file service; the core hands out
//! ids and keeps a cancel flag for each running search.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::Core;
use crate::protocol::search::{SearchId, SearchRequest, SearchResponse};
use crate::protocol::{ErrorCode, OkResponse, ProtocolError, Response};
use crate::services::SearchCancel;

/// The longest pattern a search accepts, in bytes.
const MAX_PATTERN: usize = 1000;
/// Searches that may run at once; starting another cancels the oldest.
const MAX_RUNNING: usize = 4;

#[derive(Debug, Default)]
pub(crate) struct Searches {
    last: AtomicU64,
    /// Cancel flags of searches that may still run.
    running: Mutex<BTreeMap<SearchId, SearchCancel>>,
}

impl Searches {
    fn running(&self) -> std::sync::MutexGuard<'_, BTreeMap<SearchId, SearchCancel>> {
        self.running
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

pub(crate) fn handle(core: &Core, request: SearchRequest) -> Result<Response, ProtocolError> {
    let searches = core.searches();
    match request {
        SearchRequest::Start { project, query } => {
            if query.pattern.is_empty() || query.pattern.len() > MAX_PATTERN {
                return Err(ProtocolError::new(
                    ErrorCode::BadRequest,
                    format!("the search text must have 1 to {MAX_PATTERN} bytes"),
                ));
            }
            let root = core.root(&project)?;
            let id = SearchId(searches.last.fetch_add(1, Ordering::Relaxed) + 1);
            let cancel = SearchCancel::default();
            {
                let mut running = searches.running();
                // A flag nobody else holds belongs to a search that ended.
                running.retain(|_, flag| !flag.is_alone());
                while running.len() >= MAX_RUNNING {
                    if let Some((_, oldest)) = running.pop_first() {
                        oldest.cancel();
                    }
                }
                running.insert(id, cancel.clone());
            }
            if let Err(e) = core
                .fs_service()
                .search(id, root, query, core.sink(), cancel)
            {
                searches.running().remove(&id);
                return Err(e);
            }
            Ok(Response::Search(SearchResponse::Started { id }))
        }
        SearchRequest::Cancel { id } => {
            if let Some(flag) = searches.running().remove(&id) {
                flag.cancel();
            }
            Ok(Response::Ok(OkResponse::Ok))
        }
    }
}

#[cfg(test)]
mod tests;
