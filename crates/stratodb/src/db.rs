//! The database handle and the database-wide shared state.

use crate::{
    cache::PathCache,
    constants::METADATA_TABLE_NAME,
    engine,
    error::{SdbError, SdbResult},
    Table,
};

use redb::Database;
use std::{
    collections::HashMap,
    path::Path,
    sync::{atomic::AtomicU64, Arc, Mutex, RwLock},
};

/// Capacity (in entries) of each table's path-resolution cache.
const PATH_CACHE_CAPACITY: usize = 256 * 1024;

/// State shared by every [`StratoDb`] / [`Table`] handle on one database file.
pub(crate) struct DbInner {
    pub(crate) db: Database,

    /// Bumped on every committed write. Path-cache entries are tagged with the
    /// generation they were resolved under, so a snapshot never reads another
    /// version's resolutions.
    pub(crate) generation: AtomicU64,

    /// Serializes a reader's `(begin_read + generation read)` against a writer's
    /// `(commit + generation bump)`, so the snapshot and its generation are
    /// captured atomically.
    pub(crate) version_lock: RwLock<()>,

    /// One path cache per table, created on demand and shared across handles.
    caches: Mutex<HashMap<String, Arc<PathCache>>>,
}

impl DbInner {
    fn new(db: Database) -> Self {
        Self {
            db,
            generation: AtomicU64::new(0),
            version_lock: RwLock::new(()),
            caches: Mutex::new(HashMap::new()),
        }
    }

    /// The shared path cache for `name`, creating it on first use.
    fn cache(&self, name: &str) -> SdbResult<Arc<PathCache>> {
        let cache = self
            .caches
            .lock()
            .map_err(|err| {
                let msg = format!("table cache registry poisoned while getting cache for table {name}: {err}");

                SdbError::CannotAccess(msg)
            })?
            .entry(name.to_string())
            .or_insert_with(|| Arc::new(PathCache::new(PATH_CACHE_CAPACITY)))
            .clone();

        Ok(cache)
    }
}

/// A StratoDB database: a single file holding one or more tables.
///
/// The handle is cheap to clone (it shares the underlying database) and is safe
/// to use concurrently from multiple threads.
#[derive(Clone)]
pub struct StratoDb {
    inner: Arc<DbInner>,
}

impl StratoDb {
    /// Opens the database at `path`, creating it if it does not exist.
    pub fn create(path: impl AsRef<Path>) -> SdbResult<Self> {
        let db = Database::create(path)?;
        engine::bootstrap_metadata(&db)?;

        Ok(Self {
            inner: Arc::new(DbInner::new(db)),
        })
    }

    /// Opens an existing database at `path`.
    pub fn open(path: impl AsRef<Path>) -> SdbResult<Self> {
        let db = Database::open(path)?;
        engine::bootstrap_metadata(&db)?;

        Ok(Self {
            inner: Arc::new(DbInner::new(db)),
        })
    }

    /// Returns a handle to the named table. The table is created on first write.
    pub fn open_table(&self, name: &str) -> SdbResult<Table> {
        if name.is_empty() {
            return Err(SdbError::InvalidTableName("table name must not be empty".into()));
        }

        if name == METADATA_TABLE_NAME {
            return Err(SdbError::InvalidTableName(format!("'{name}' is reserved")));
        }

        let cache = self.inner.cache(name)?;
        Ok(Table::new(Arc::clone(&self.inner), name.to_string(), cache))
    }
}
