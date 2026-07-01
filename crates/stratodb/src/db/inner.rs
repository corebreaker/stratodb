use crate::{
    cache::PathCache,
    error::{SdbError, SdbResult},
    index::registry::IndexEntry,
};

use redb::Database;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
        Mutex,
        RwLock,
    },
};

/// A table's parsed index set, tagged with the schema generation it was read
/// under (see [`DbInner::schema_gen`]).
type CachedIndexSet = (u64, Arc<[IndexEntry]>);

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

    /// Bumped only when the index *schema* changes (an index is created or
    /// dropped). Index sets cached in [`index_sets`](Self::index_sets) are tagged
    /// with the schema generation they were read under, so a write transaction can
    /// reuse a table's parsed index set without reopening `$metadata` on every
    /// mutation — the common case, since index DDL is rare. The committed registry
    /// is stable while a (serialized) write transaction runs, so a cached set is
    /// valid as long as the schema generation has not moved.
    schema_gen: AtomicU64,

    /// Serializes a reader's `(begin_read + generation read)` against a writer's
    /// `(commit + generation bump)`, so the snapshot and its generation are
    /// captured atomically.
    version_lock: RwLock<()>,

    /// One path cache per table, created on demand and shared across handles.
    caches: Mutex<HashMap<String, Arc<PathCache>>>,

    /// Per-table parsed index sets, each tagged with the schema generation it was
    /// loaded under (see [`schema_gen`](Self::schema_gen)). Lets a write
    /// transaction skip the `$metadata` open + registry decode on every mutation.
    index_sets: Mutex<HashMap<String, CachedIndexSet>>,
}

impl DbInner {
    pub(super) fn new(db: Database) -> Self {
        Self {
            db,
            generation: AtomicU64::new(0),
            schema_gen: AtomicU64::new(0),
            version_lock: RwLock::new(()),
            caches: Mutex::new(HashMap::new()),
            index_sets: Mutex::new(HashMap::new()),
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

    /// This table's parsed index set, served from the cross-transaction cache when
    /// the schema generation has not moved since it was loaded; otherwise `load`
    /// (which opens `$metadata`) repopulates it. A write transaction is serialized
    /// against index DDL, so the committed registry — and thus the cached set — is
    /// stable for the life of the caller's transaction.
    pub(crate) fn cached_indexes(
        &self,
        table: &str,
        load: impl FnOnce() -> SdbResult<Vec<IndexEntry>>,
    ) -> SdbResult<Arc<[IndexEntry]>> {
        let current = self.schema_gen.load(Ordering::Acquire);

        {
            let cache = self.index_sets.lock().map_err(|err| {
                SdbError::CannotAccess(format!(
                    "index-set cache poisoned while getting for table {table}: {err}"
                ))
            })?;

            if let Some((generation, set)) = cache.get(table)
                && *generation == current
            {
                return Ok(Arc::clone(set));
            }
        }

        let set: Arc<[IndexEntry]> = Arc::from(load()?);

        let mut cache = self.index_sets.lock().map_err(|err| {
            SdbError::CannotAccess(format!(
                "index-set cache poisoned while putting for table {table}: {err}"
            ))
        })?;

        cache.insert(table.to_string(), (current, Arc::clone(&set)));

        Ok(set)
    }

    /// Invalidates every cached index set (an index was created or dropped). Cheap
    /// and rare; the next mutation on each table reloads its set from `$metadata`.
    pub(crate) fn bump_schema_gen(&self) {
        self.schema_gen.fetch_add(1, Ordering::Release);
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

#[cfg(test)]
mod tests {
    use super::*;
    use redb::backends::InMemoryBackend;
    use std::panic::{catch_unwind, AssertUnwindSafe};

    fn inner() -> DbInner {
        let db = Database::builder().create_with_backend(InMemoryBackend::new()).unwrap();

        DbInner::new(db)
    }

    /// Poisons `mutex` by panicking while its lock is held (panic hook silenced).
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
    fn cache_reports_a_poisoned_registry() {
        let inner = inner();
        poison(&inner.caches);

        assert!(matches!(inner.cache("t"), Err(SdbError::CannotAccess(_))));
    }

    #[test]
    fn cached_indexes_reports_a_poisoned_mutex_on_either_lock() {
        // Poisoned before the call: the first lock fails.
        let first = inner();
        poison(&first.index_sets);
        assert!(first.cached_indexes("t", || Ok(vec![])).is_err());

        // Poisoned by `load`, between the two locks: the second (put) lock fails.
        // This depends on `cached_indexes` releasing the first (get) lock *before*
        // calling `load` — as it does today, the get lock lives in an inner block
        // that drops before `load` runs. Were the two acquisitions ever merged into
        // one guard held across `load`, poisoning here would fail that single lock
        // and this case would silently stop covering the second `CannotAccess` arm.
        let second = inner();
        let result = second.cached_indexes("t", || {
            poison(&second.index_sets);

            Ok(vec![])
        });
        assert!(result.is_err());
    }
}
