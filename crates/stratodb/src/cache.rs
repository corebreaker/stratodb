//! Bounded LRU caches mapping resolved paths to primary keys, and packed-entity
//! keys to their decoded blobs.
//!
//! Both mappings are only valid for one committed database version, so entries
//! are keyed by `(generation, …)`. A write commit bumps the generation (see
//! [`crate::db`]); stale entries are then simply never looked up again and are
//! LRU-evicted — they are never served for the wrong snapshot. (A packed entity's
//! blob is immutable within a generation: any edit replaces the entity under a
//! fresh key or rewrites it behind a generation bump.)

use crate::{engine::MemNodes, path::SPath, SdbError, SdbResult, Skey};
use lru::LruCache;
use std::{num::NonZeroUsize, sync::Arc, sync::Mutex};

/// A thread-safe, bounded set of per-table caches shared by all of that table's
/// transactions: `(generation, SPath) -> Skey` path resolutions and
/// `(generation, Skey) -> MemNodes` decoded packed-entity blobs.
pub(crate) struct PathCache {
    entries: Mutex<LruCache<(u64, SPath), Skey>>,
    blobs:   Mutex<LruCache<(u64, Skey), Arc<MemNodes>>>,
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
    pub(crate) fn get_blob(&self, generation: u64, key: Skey) -> SdbResult<Option<Arc<MemNodes>>> {
        let blob = self
            .blobs
            .lock()
            .map_err(|err| SdbError::CannotAccess(format!("blob cache mutex poisoned while getting: {err}")))?
            .get(&(generation, key))
            .cloned();

        Ok(blob)
    }

    /// Records that `key` decodes to `blob` at `generation`.
    pub(crate) fn put_blob(&self, generation: u64, key: Skey, blob: Arc<MemNodes>) -> SdbResult<()> {
        self.blobs
            .lock()
            .map_err(|err| SdbError::CannotAccess(format!("blob cache mutex poisoned while putting: {err}")))?
            .put((generation, key), blob);

        Ok(())
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
