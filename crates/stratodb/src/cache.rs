//! A bounded LRU cache mapping resolved paths to primary keys.
//!
//! A `SPath -> Skey` mapping is only valid for one committed database version, so
//! entries are keyed by `(generation, path)`. A write commit bumps the generation
//! (see [`crate::db`]); stale entries are then simply never looked up again and
//! are LRU-evicted — they are never served for the wrong snapshot.

use crate::{path::SPath, SdbError, SdbResult, Skey};
use lru::LruCache;
use std::{num::NonZeroUsize, sync::Mutex};

/// A thread-safe, bounded `(generation, SPath) -> Skey` cache held by a table and
/// shared by all of that table's transactions.
pub(crate) struct PathCache {
    entries: Mutex<LruCache<(u64, SPath), Skey>>,
}

impl PathCache {
    /// Creates a cache holding at most `capacity` entries.
    ///
    /// `capacity == 0` is coerced up to 1 rather than rejected: an `LruCache`
    /// requires a `NonZeroUsize`, and a one-entry cache is a harmless, valid
    /// (if pointless) configuration — not a condition worth erroring or
    /// asserting on.
    pub(crate) fn new(capacity: usize) -> Self {
        let capacity = NonZeroUsize::new(capacity).unwrap_or(NonZeroUsize::MIN);

        Self {
            entries: Mutex::new(LruCache::new(capacity)),
        }
    }

    /// Returns the cached key for `path` at `generation`, if present.
    pub(crate) fn get(&self, generation: u64, path: &SPath) -> SdbResult<Option<Skey>> {
        let key = self
            .entries
            .lock()
            .map_err(|err| {
                let msg = format!("path cache mutex poisoned while getting key: {err}");

                SdbError::CannotAccess(msg)
            })?
            .get(&(generation, path.clone()))
            .copied();

        Ok(key)
    }

    /// Records that `path` resolves to `key` at `generation`.
    pub(crate) fn put(&self, generation: u64, path: &SPath, key: Skey) -> SdbResult<()> {
        self.entries
            .lock()
            .map_err(|err| {
                let msg = format!("path cache mutex poisoned while putting key: {err}");

                SdbError::CannotAccess(msg)
            })?
            .put((generation, path.clone()), key);

        Ok(())
    }
}
