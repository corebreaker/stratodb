//! Bounded LRU caches mapping resolved paths to primary keys, and packed-entity
//! keys to their decoded blobs.
//!
//! Both mappings are only valid for one committed database version, so entries
//! are keyed by `(generation, …)`. A write commit bumps the generation (see
//! [`crate::db`]); stale entries are then simply never looked up again and are
//! LRU-evicted — they are never served for the wrong snapshot. (A packed entity's
//! blob is immutable within a generation: any edit replaces the entity under a
//! fresh key or rewrites it behind a generation bump.)

use crate::{engine::ArchivedNodes, path::SPath, SdbError, SdbResult, Skey};
use lru::LruCache;
use std::{num::NonZeroUsize, sync::Arc, sync::Mutex};

/// A thread-safe, bounded set of per-table caches shared by all of that table's
/// transactions: `SPath -> (generation, Skey)` path resolutions and
/// `(generation, Skey) -> MemNodes` decoded packed-entity blobs.
///
/// The path cache stores the resolving generation in the *value*, not the key, so
/// a lookup borrows the caller's `&SPath` directly instead of cloning it into a
/// composite key on every hot-path read; a stale-generation hit reads as a miss
/// and is re-resolved (and overwritten) under the caller's own snapshot.
pub(crate) struct PathCache {
    entries: Mutex<LruCache<SPath, (u64, Skey)>>,
    blobs:   Mutex<LruCache<(u64, Skey), Arc<ArchivedNodes>>>,
}

impl PathCache {
    /// Creates caches holding at most `capacity` path entries and `blob_capacity`
    /// decoded blobs.
    ///
    /// A zero capacity is coerced up to 1 rather than rejected: an `LruCache`
    /// requires a `NonZeroUsize`, and a one-entry cache is a harmless, valid
    /// (if pointless) configuration — not a condition worth erroring or
    /// asserting on.
    pub(crate) fn new(capacity: usize, blob_capacity: usize) -> Self {
        let capacity = NonZeroUsize::new(capacity).unwrap_or(NonZeroUsize::MIN);
        let blob_capacity = NonZeroUsize::new(blob_capacity).unwrap_or(NonZeroUsize::MIN);

        Self {
            entries: Mutex::new(LruCache::new(capacity)),
            blobs:   Mutex::new(LruCache::new(blob_capacity)),
        }
    }

    /// Returns the cached decoded blob for `key` at `generation`, if present.
    pub(crate) fn get_blob(&self, generation: u64, key: Skey) -> SdbResult<Option<Arc<ArchivedNodes>>> {
        let blob = self
            .blobs
            .lock()
            .map_err(|err| SdbError::CannotAccess(format!("blob cache mutex poisoned while getting: {err}")))?
            .get(&(generation, key))
            .cloned();

        Ok(blob)
    }

    /// Records that `key` decodes to `blob` at `generation`.
    pub(crate) fn put_blob(&self, generation: u64, key: Skey, blob: Arc<ArchivedNodes>) -> SdbResult<()> {
        self.blobs
            .lock()
            .map_err(|err| SdbError::CannotAccess(format!("blob cache mutex poisoned while putting: {err}")))?
            .put((generation, key), blob);

        Ok(())
    }

    /// Returns the cached key for `path` at `generation`, if present. A hit tagged
    /// with another generation is treated as a miss (the resolution belongs to a
    /// different committed version).
    pub(crate) fn get(&self, generation: u64, path: &SPath) -> SdbResult<Option<Skey>> {
        let key = self
            .entries
            .lock()
            .map_err(|err| {
                let msg = format!("path cache mutex poisoned while getting key: {err}");

                SdbError::CannotAccess(msg)
            })?
            .get(path)
            .and_then(|(tagged, key)| (*tagged == generation).then_some(*key));

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
            .put(path.clone(), (generation, key));

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::{catch_unwind, AssertUnwindSafe};

    /// Poisons `mutex` by panicking while its lock is held, silencing the panic
    /// hook so the deliberate panic does not clutter test output.
    fn poison<T>(mutex: &Mutex<T>) {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));

        let _ = catch_unwind(AssertUnwindSafe(|| {
            let _guard = mutex.lock().unwrap();

            panic!("poison the mutex");
        }));

        std::panic::set_hook(previous);
    }

    #[test]
    fn get_and_put_report_a_poisoned_path_mutex() {
        let cache = PathCache::new(4, 4);
        poison(&cache.entries);

        assert!(matches!(cache.get(0, &SPath::root()), Err(SdbError::CannotAccess(_))));
        assert!(matches!(
            cache.put(0, &SPath::root(), Skey::ROOT),
            Err(SdbError::CannotAccess(_))
        ));
    }
}
