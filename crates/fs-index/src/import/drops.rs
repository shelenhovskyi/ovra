//! The paths of the last drop onto the window, kept where no client can
//! write them. The window gets a random id for them; an import takes the
//! drop by its id, once, within `DROP_TTL`.

use std::path::PathBuf;
use std::time::{Duration, Instant};

/// How long a drop can be imported after it happened.
pub const DROP_TTL: Duration = Duration::from_secs(60);

/// Holds the last drop only: a new one replaces it.
#[derive(Debug, Default)]
pub struct Drops {
    last: Option<Dropped>,
}

/// A drop taken out of `Drops`.
#[derive(Debug)]
pub struct Dropped {
    id: String,
    paths: Vec<PathBuf>,
    expires: Instant,
}

impl Dropped {
    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }
}

impl Drops {
    pub fn new() -> Self {
        Self::default()
    }

    /// Keeps `paths`, dropped at `now`, in place of any earlier drop, and
    /// returns their new id.
    pub fn put(&mut self, paths: Vec<PathBuf>, now: Instant) -> String {
        let id = format!("{:032x}", rand::random::<u128>());
        self.last = Some(Dropped {
            id: id.clone(),
            paths,
            expires: now + DROP_TTL,
        });
        id
    }

    /// Takes the drop `id` if it is the last one and has not expired at
    /// `now`. A second take of the same id finds nothing.
    pub fn take(&mut self, id: &str, now: Instant) -> Option<Dropped> {
        let last = self.last.take()?;
        if last.id != id {
            self.last = Some(last);
            return None;
        }
        (now < last.expires).then_some(last)
    }

    /// Puts back a drop that was taken but not used (the person is asked
    /// about a name clash first), keeping its expiry, unless a newer drop
    /// arrived meanwhile.
    pub fn put_back(&mut self, dropped: Dropped) {
        if self.last.is_none() {
            self.last = Some(dropped);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drop_is_taken_once_by_its_id_before_it_expires() {
        let now = Instant::now();
        let mut drops = Drops::new();
        let id = drops.put(vec![PathBuf::from("/a")], now);
        assert!(drops.take("other", now).is_none());
        let taken = drops.take(&id, now).expect("taken");
        assert_eq!(taken.paths(), [PathBuf::from("/a")]);
        assert!(drops.take(&id, now).is_none(), "single use");

        // Put back after a clash question, it keeps its expiry.
        drops.put_back(taken);
        assert!(drops.take(&id, now + DROP_TTL).is_none(), "expired");
        assert!(drops.take(&id, now).is_none(), "an expired take drops it");

        // A newer drop replaces the last one, also over a put back.
        let first = drops.put(vec![PathBuf::from("/a")], now);
        let taken = drops.take(&first, now).expect("taken");
        let second = drops.put(vec![PathBuf::from("/b")], now);
        assert_ne!(first, second);
        drops.put_back(taken);
        assert!(drops.take(&first, now).is_none());
        assert!(drops.take(&second, now).is_some());
    }
}
