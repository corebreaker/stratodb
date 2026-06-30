use crate::{
    cache::PathCache,
    error::{SdbError, SdbResult},
};

use redb::Database;
use std::{
    collections::HashMap,
    sync::{atomic::AtomicU64, Arc, Mutex, RwLock},
};

/// Capacity (in entries) of each table's path-resolution cache.
const PATH_CACHE_CAPACITY: usize = 256 * 1024;

/// Capacity (in entries) of each table's decoded packed-blob cache. Smaller than
/// the path cache: each entry holds a whole decoded entity, not a single key.
const BLOB_CACHE_CAPACITY: usize = 64 * 1024;

/// State shared by every [`StratoDb`] / [`Table`] handle on one database file.
pub(crate) struct DbInner {
    db: Database,

    /// Bumped on every committed write. Path-cache entries are tagged with the
    /// generation they were resolved under, so a snapshot never reads another
    /// version's resolutions.
    generation: AtomicU64,

    /// Serializes a reader's `(begin_read + generation read)` against a writer's
    /// `(commit + generation bump)`, so the snapshot and its generation are
    /// captured atomically.
    version_lock: RwLock<()>,

    /// One path cache per table, created on demand and shared across handles.
    caches: Mutex<HashMap<String, Arc<PathCache>>>,
}

impl DbInner {
    pub(super) fn new(db: Database) -> Self {
        Self {
            db,
            generation: AtomicU64::new(0),
            version_lock: RwLock::new(()),
            caches: Mutex::new(HashMap::new()),
        }
    }

    /// The shared path cache for `name`, creating it on first use.
    pub(super) fn cache(&self, name: &str) -> SdbResult<Arc<PathCache>> {
        let cache = self
            .caches
            .lock()
            .map_err(|err| {
                let msg = format!("table cache registry poisoned while getting cache for table {name}: {err}");

                SdbError::CannotAccess(msg)
            })?
            .entry(name.to_string())
            .or_insert_with(|| Arc::new(PathCache::new(PATH_CACHE_CAPACITY, BLOB_CACHE_CAPACITY)))
            .clone();

        Ok(cache)
    }

    pub(crate) fn db(&self) -> &Database {
        &self.db
    }

    pub(crate) fn generation(&self) -> &AtomicU64 {
        &self.generation
    }

    pub(crate) fn version_lock(&self) -> &RwLock<()> {
        &self.version_lock
    }
}
