//! Direct mode across restarts: each tracked session's baseline, which
//! files are its agent's, its checkpoints and what a reject or go-back can
//! undo, as one JSON document in the [`SessionStore`].
//!
//! It is saved at turn edges and after every change the person asks for
//! (go back, approve, reject, their undos), not after each tool: what an
//! agent did since the last save then counts as changed outside, which
//! only keeps files. Tools and open intervals are not kept; an agent no
//! longer runs after a restart.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Tracker, Undo, Writer, lock, turns::Turns};
use crate::protocol::agents::SessionId;
use crate::protocol::projects::ProjectId;
use crate::services::SessionStore;

/// Saves the writers' states, each as it was last handed over.
#[derive(Debug)]
pub(super) struct Keeper {
    store: Arc<dyn SessionStore>,
    /// Held while the document is written, so saves keep their order.
    kept: Mutex<BTreeMap<SessionId, Value>>,
}

/// One writer, as saved.
#[derive(Debug, Deserialize)]
pub(super) struct Kept {
    pub(super) project: ProjectId,
    pub(super) dir: PathBuf,
    pub(super) tracker: Option<Tracker>,
    pub(super) undo: Option<Undo>,
    #[serde(default)]
    pub(super) turns: Turns,
}

#[derive(Serialize)]
struct KeptRef<'a> {
    project: &'a ProjectId,
    dir: &'a PathBuf,
    tracker: &'a Option<Tracker>,
    undo: &'a Option<Undo>,
    turns: &'a Turns,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Document {
    writers: BTreeMap<SessionId, Value>,
}

impl Keeper {
    /// The writers saved in `store`. One that cannot be read is left out,
    /// and so goes with the next save: its files then count as changed
    /// outside, as before checkpoints were kept.
    pub(super) fn load(store: Arc<dyn SessionStore>) -> (Self, Vec<(SessionId, Kept)>) {
        let document: Document = store
            .load_direct()
            .ok()
            .flatten()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
        let mut kept = BTreeMap::new();
        let mut loaded = Vec::new();
        for (session, value) in document.writers {
            if let Ok(writer) = serde_json::from_value::<Kept>(value.clone()) {
                kept.insert(session.clone(), value);
                loaded.push((session, writer));
            }
        }
        let keeper = Self {
            store,
            kept: Mutex::new(kept),
        };
        (keeper, loaded)
    }

    /// Saves `session` as `value`, or without it.
    fn put(&self, session: &SessionId, value: Option<Value>) {
        let mut kept = lock(&self.kept);
        let changed = match value {
            Some(value) => kept.insert(session.clone(), value.clone()) != Some(value),
            None => kept.remove(session).is_some(),
        };
        if changed {
            self.save(&kept);
        }
    }

    pub(super) fn forget(&self, sessions: &[SessionId]) {
        let mut kept = lock(&self.kept);
        let before = kept.len();
        kept.retain(|id, _| !sessions.contains(id));
        if kept.len() != before {
            self.save(&kept);
        }
    }

    /// A failed save only means the state is not kept.
    fn save(&self, kept: &BTreeMap<SessionId, Value>) {
        #[derive(Serialize)]
        struct Ref<'a> {
            writers: &'a BTreeMap<SessionId, Value>,
        }
        if let Ok(json) = serde_json::to_string(&Ref { writers: kept }) {
            let _ = self.store.save_direct(&json);
        }
    }
}

impl Writer {
    /// Saves the writer with `tracker`, its tracker (whose lock the caller
    /// holds). One with nothing to go back to or undo is not kept.
    pub(super) fn keep(&self, tracker: &Option<Tracker>) {
        let undo = lock(&self.undo);
        let value = if tracker.is_none() && undo.is_none() {
            None
        } else {
            let turns = lock(&self.turns);
            serde_json::to_value(KeptRef {
                project: &self.project,
                dir: &self.dir,
                tracker,
                undo: &undo,
                turns: &turns,
            })
            .ok()
        };
        drop(undo);
        self.keeper.put(&self.session, value);
    }
}

/// `Vec<RelPath>` as the paths' text, checked again when read.
pub(super) mod rel_paths {
    use serde::de::Error as _;
    use serde::{Deserialize, Deserializer, Serializer};

    use crate::path::RelPath;

    pub(crate) fn serialize<S: Serializer>(paths: &[RelPath], s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(paths.iter().map(RelPath::as_str))
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<RelPath>, D::Error> {
        Vec::<String>::deserialize(d)?
            .iter()
            .map(|p| RelPath::parse(p).map_err(|e| D::Error::custom(e.message)))
            .collect()
    }
}
