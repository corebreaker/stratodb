//! A handle to a single table, from which transactions are started.

use crate::{
    cache::PathCache,
    db::DbInner,
    error::{SdbError, SdbResult},
    txn::{ReadTxn, WriteTxn},
};

use redb::ReadableDatabase;
use std::sync::{atomic::Ordering, Arc};

/// A lightweight handle to a named table.
///
/// Cloning is cheap. Reads run concurrently; writes are serialized (a single
/// writer at a time), matching the underlying engine's transaction model. The
/// handle carries a shared [`PathCache`] so a transaction's accessors reuse
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
            .version_lock
            .read()
            .map_err(|err| SdbError::CannotAccess(format!("version lock poisoned: {err}")))?;

        let txn = self.inner.db.begin_read()?;
        let generation = self.inner.generation.load(Ordering::Acquire);
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
        let txn = self.inner.db.begin_write()?;

        Ok(WriteTxn::new(txn, self.name.clone(), Arc::clone(&self.inner)))
    }
}
