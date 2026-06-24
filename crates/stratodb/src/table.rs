//! A handle to a single table, from which transactions are started.

use crate::{
    error::SdbResult,
    txn::{ReadTxn, WriteTxn},
};

use redb::{Database, ReadableDatabase};
use std::sync::Arc;

/// A lightweight handle to a named table.
///
/// Cloning is cheap. Reads run concurrently; writes are serialized (a single
/// writer at a time), matching the underlying engine's transaction model.
#[derive(Clone)]
pub struct Table {
    db:   Arc<Database>,
    name: String,
}

impl Table {
    pub(crate) fn new(db: Arc<Database>, name: String) -> Self {
        Self {
            db,
            name,
        }
    }

    /// The table's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Begins a read transaction. Multiple read transactions may run
    /// concurrently with each other and with a single writer.
    pub fn read(&self) -> SdbResult<ReadTxn> {
        let txn = self.db.begin_read()?;
        Ok(ReadTxn::new(txn, self.name.clone()))
    }

    /// Begins a write transaction. Only one write transaction may be active at a
    /// time across the whole database.
    pub fn write(&self) -> SdbResult<WriteTxn> {
        let txn = self.db.begin_write()?;
        Ok(WriteTxn::new(txn, self.name.clone()))
    }
}
