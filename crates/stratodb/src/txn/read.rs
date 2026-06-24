//! Opaque read and write transactions.

use crate::{
    data::{Scalar, SValue},
    engine::{self, TableKey, TableValue},
    error::SdbResult,
    node::NodeKind,
    path::SPath,
    tree,
};

use redb::{ReadOnlyTable, ReadTransaction, TableError};

/// A read-only view of a table at a consistent point in time.
pub struct ReadTxn {
    txn:   ReadTransaction,
    table: String,
}

impl ReadTxn {
    pub(crate) fn new(txn: ReadTransaction, table: String) -> Self {
        Self {
            txn,
            table,
        }
    }

    /// Opens the underlying table, or `None` if it has never been written.
    fn open(&self) -> SdbResult<Option<ReadOnlyTable<TableKey, TableValue>>> {
        match self.txn.open_table(engine::data_def(&self.table)) {
            Ok(table) => Ok(Some(table)),
            Err(TableError::TableDoesNotExist(_)) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// Reads the value at `path`, decoded as `V`.
    pub fn get<V: SValue>(&self, path: &str) -> SdbResult<Option<V>> {
        let path = SPath::parse(path)?;
        let Some(table) = self.open()? else {
            return Ok(None);
        };

        match tree::get_scalar(&table, &path)? {
            Some(scalar) => Ok(Some(V::from_scalar(&scalar)?)),
            None => Ok(None),
        }
    }

    /// Reads the raw scalar at `path`.
    pub fn get_scalar(&self, path: &str) -> SdbResult<Option<Scalar>> {
        let path = SPath::parse(path)?;
        let Some(table) = self.open()? else {
            return Ok(None);
        };

        tree::get_scalar(&table, &path)
    }

    /// Reports the kind of node at `path`, if any.
    pub fn kind(&self, path: &str) -> SdbResult<Option<NodeKind>> {
        let path = SPath::parse(path)?;
        let Some(table) = self.open()? else {
            return Ok(None);
        };

        tree::kind(&table, &path)
    }

    /// Returns whether a node exists at `path`.
    pub fn exists(&self, path: &str) -> SdbResult<bool> {
        Ok(self.kind(path)?.is_some())
    }
}
