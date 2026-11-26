//! The paths of the last drop onto the window, kept where no client can
//! write them. The window gets a random id for them; an import takes the
//! drop by its id, once, within `DROP_TTL`.

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// How long a drop can be imported after it happened.
pub(crate) const DROP_TTL: Duration = Duration::from_secs(60);

/// Holds the last drop only: a new one replaces it.
#[derive(Debug, Default)]
pub struct Drops {
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    last: Option<Dropped>,
    /// The id of the newest drop, also while it is taken.
    newest: Option<String>,
}

/// A drop taken out of `Drops`.
#[derive(Debug)]
pub(crate) struct Dropped {
    id: String,
    pub(crate) paths: Vec<PathBuf>,
    expires: Instant,
    /// The clashing names the person was asked about.
    pub(crate) shown: Vec<String>,
}

impl Drops {
    pub fn new() -> Self {
        Self::default()
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Keeps `paths`, dropped at `now`, in place of any earlier drop, and
    /// returns their new id.
    pub fn put(&self, paths: Vec<PathBuf>, now: Instant) -> String {
        let id = format!("{:032x}", rand::random::<u128>());
        let mut state = self.state();
        state.newest = Some(id.clone());
        state.last = Some(Dropped {
            id: id.clone(),
            paths,
            expires: now + DROP_TTL,
            shown: Vec::new(),
        });
        id
    }

    /// Takes the drop `id` if it is the last one and has not expired at
    /// `now`. A second take of the same id finds nothing.
    pub(crate) fn take(&self, id: &str, now: Instant) -> Option<Dropped> {
        let mut state = self.state();
        if state.last.as_ref().is_none_or(|last| last.id != id) {
            return None;
        }
        let last = state.last.take()?;
        (now < last.expires).then_some(last)
    }

    /// Puts back a drop that was taken but not used (the person is asked
    /// about a name clash first), keeping its expiry, unless a newer drop
    /// was made since.
    pub(crate) fn put_back(&self, dropped: Dropped) {
        let mut state = self.state();
        if state.newest.as_deref() == Some(dropped.id.as_str()) && state.last.is_none() {
            state.last = Some(dropped);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drop_is_taken_once_by_its_id_before_it_expires() {
        let now = Instant::now();
        let drops = Drops::new();
        let id = drops.put(vec![PathBuf::from("/a")], now);
        assert!(drops.take("other", now).is_none());
        let taken = drops.take(&id, now).expect("taken");
        assert_eq!(taken.paths, [PathBuf::from("/a")]);
        assert!(drops.take(&id, now).is_none(), "single use");

        // Put back after a clash question, it keeps its expiry.
        drops.put_back(taken);
        assert!(drops.take(&id, now + DROP_TTL).is_none(), "expired");
        assert!(drops.take(&id, now).is_none(), "an expired take drops it");
    }

    #[test]
    fn a_newer_drop_wins_over_a_put_back() {
        let now = Instant::now();
        let drops = Drops::new();
        let first = drops.put(vec![PathBuf::from("/a")], now);
        let taken = drops.take(&first, now).expect("taken");
        let second = drops.put(vec![PathBuf::from("/b")], now);
        assert_ne!(first, second);
        drops.put_back(taken);
        assert!(drops.take(&first, now).is_none());

        // Also when the newer one was used up meanwhile.
        let first = drops.put(vec![PathBuf::from("/a")], now);
        let taken = drops.take(&first, now).expect("taken");
        let second = drops.put(vec![PathBuf::from("/b")], now);
        assert!(drops.take(&second, now).is_some());
        drops.put_back(taken);
        assert!(drops.take(&first, now).is_none());
    }
}
