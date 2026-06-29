//! A handle to a single table, from which transactions are started.

use crate::{
    cache::PathCache,
    db::DbInner,
    engine::{self, META_TABLE},
    error::{SdbError, SdbResult},
    index::{maintenance, registry, IndexDef, SIndexed},
    path::SPath,
    txn::{ReadTxn, WriteTxn},
};

use redb::ReadableDatabase;
use std::sync::{atomic::Ordering, Arc};

/// A lightweight handle to a named table.
///
/// Cloning is cheap. Reads run concurrently; writes are serialized (a single
/// writer at a time), matching the underlying engine's transaction model. The
/// handle carries a shared `PathCache` so a transaction's accessors reuse
/// resolved paths.
#[derive(Clone)]
pub struct Table {
    inner: Arc<DbInner>,
    name:  String,
    cache: Arc<PathCache>,
}

impl Table {
    pub(crate) fn new(inner: Arc<DbInner>, name: String, cache: Arc<PathCache>) -> Self {
        Self {
            inner,
            name,
            cache,
        }
    }

    /// The table's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Begins a read transaction. Multiple read transactions may run concurrently
    /// with each other and with a single writer.
    pub fn read(&self) -> SdbResult<ReadTxn> {
        // Capture the snapshot and its matching generation atomically with respect
        // to commits, so cached resolutions are never attributed to the wrong one.
        let guard = self
            .inner
            .version_lock()
            .read()
            .map_err(|err| SdbError::CannotAccess(format!("version lock poisoned: {err}")))?;

        let txn = self.inner.db().begin_read()?;
        let generation = self.inner.generation().load(Ordering::Acquire);
        drop(guard);

        Ok(ReadTxn::new(
            txn,
            self.name.clone(),
            generation,
            Arc::clone(&self.cache),
        ))
    }

    /// Begins a write transaction. Only one write transaction may be active at a
    /// time across the whole database.
    pub fn write(&self) -> SdbResult<WriteTxn> {
        let txn = self.inner.db().begin_write()?;

        Ok(WriteTxn::new(txn, self.name.clone(), Arc::clone(&self.inner)))
    }

    /// Registers a secondary index on this table and back-fills it.
    ///
    /// Idempotent for an identical definition; errors with
    /// [`SchemaMismatch`](crate::SdbError::SchemaMismatch) if `def.name` already
    /// names a different index here. On first creation, every pre-existing entity
    /// the index matches is indexed too, so the index is correct whether data was
    /// written before or after — and creating a unique index over duplicate data
    /// fails with [`UniqueViolation`](crate::SdbError::UniqueViolation).
    pub fn create_index(&self, def: &IndexDef) -> SdbResult<()> {
        let txn = self.inner.db().begin_write()?;

        // Register; on a fresh creation, recover the entry (it carries the new id)
        // so the back-fill below can build its keys.
        let new_entry = {
            let mut meta = txn.open_table(META_TABLE)?;
            if registry::create(&mut meta, &self.name, def)? {
                registry::lookup(&meta, &self.name, def.name())?
            } else {
                None
            }
        };

        // Back-fill: index every entity the new index already matches.
        if let Some(entry) = new_entry {
            let mut data = txn.open_table(engine::data_def(&self.name))?;
            maintenance::insert(&mut data, std::slice::from_ref(&entry), &SPath::root())?;
        }

        txn.commit()?;

        Ok(())
    }

    /// Registers every index that `T` declares (via `#[strato(index(...))]`),
    /// scoping each to `pattern`. A shorthand for calling [`create_index`](Self::create_index) for each
    /// of [`T::index_defs`](SIndexed::index_defs); each is idempotent and
    /// back-filled.
    pub fn create_indexes<T: SIndexed>(&self, pattern: &str) -> SdbResult<()> {
        for def in T::index_defs(pattern) {
            self.create_index(&def)?;
        }

        Ok(())
    }

    /// Returns the definition of the named index on this table, if it exists.
    pub fn index_def(&self, name: &str) -> SdbResult<Option<IndexDef>> {
        let txn = self.inner.db().begin_read()?;
        let meta = txn.open_table(META_TABLE)?;

        Ok(registry::lookup(&meta, &self.name, name)?.map(|entry| entry.into_def()))
    }
}
